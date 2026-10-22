//! The account's search index as the engines see it: the slot holding the open
//! [`SearchIndex`] (`search.redb` next to `chat.redb`) and the write hooks the chat and group
//! engines call after every accepted change. Reconcile on open and the query facade read the
//! chat and group stores, so they stay above both engines (p2pcore). See `docs/search.md`.
//!
//! A failure here never fails the change that called it: hooks log and go on, and the next
//! reconcile repairs the index from the stores.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use chat_core::doc::MsgRec;
use group_core::state::{Item, PostDiff};
use parking_lot::Mutex;
use search_core::{DocRow, INDEX_DOMAIN, Keys, Mark, STORE_DOMAIN, SearchIndex};

/// The index source of a 1:1 conversation.
pub fn dm_src(pair: &str) -> String {
    format!("dm.{pair}")
}

/// The index source of a group.
pub fn group_src(gid: &str) -> String {
    format!("g.{gid}")
}

/// What a 1:1 message contributes to the index: its text and the name of a file it carries.
pub fn dm_row(did: &str, pair: &str, day: &str, rec: &MsgRec) -> DocRow {
    let mut t = if rec.deleted { String::new() } else { rec.text.clone() };
    if !rec.deleted
        && let Some(f) = rec.file.as_ref().filter(|f| f.kind != "voice")
    {
        if !t.is_empty() {
            t.push('\n');
        }
        t.push_str(&f.name);
    }
    DocRow { src: dm_src(pair), conv: did.to_string(), day: day.to_string(), id: rec.id.clone(), at: rec.at, author: rec.author.clone(), text: t }
}

/// What a group post contributes to the index.
pub fn group_row(gid: &str, day: &str, i: &Item) -> DocRow {
    DocRow { src: group_src(gid), conv: gid.to_string(), day: day.to_string(), id: i.id.clone(), at: i.at, author: i.author.clone(), text: i.text.clone() }
}

/// Opens (creating) the index file at `path` with the phrase's keys: waits for a previous owner
/// of the file and makes it private, also when a stale file is replaced.
fn open_index(path: &Path, phrase: &str) -> Result<SearchIndex, search_core::Error> {
    use node_runtime::rekey::phrase_key;
    let keys = Keys { store: phrase_key(STORE_DOMAIN, phrase), index: phrase_key(INDEX_DOMAIN, phrase) };
    let open_store = |p: &Path| -> Result<storage::Store, search_core::Error> {
        let store = node_runtime::store::open_waiting(p).map_err(|e| match e {
            node_runtime::Error::Io(s) => search_core::Error::Io(s),
            e => search_core::Error::Io(e.to_string()),
        })?;
        node_runtime::store::set_private(p);
        Ok(store)
    };
    SearchIndex::open(path, keys, &open_store)
}

fn log(e: search_core::Error) {
    tracing::warn!("search index: {e}");
}

/// The slot of the selected account's index: empty while locked, between accounts, or when the
/// index failed to open (search is then off for that session; nothing else depends on it).
#[derive(Default)]
pub struct SearchIx {
    slot: Mutex<Option<Arc<SearchIndex>>>,
    /// Test switch: the next `n` hook writes are skipped (`search_skip_next_writes_for_test`).
    skip: AtomicU32,
}

impl SearchIx {
    pub fn new() -> Self {
        Self::default()
    }

    // ---- lifecycle -------------------------------------------------------------------------

    /// Opens the account's index next to `chat.redb` (called by the chat store's open). A
    /// failure leaves search off for this session.
    pub fn open(&self, dir: &Path, phrase: &str) {
        let mut slot = self.slot.lock();
        if slot.as_ref().is_some_and(|s| !s.is_closed()) {
            return;
        }
        match open_index(&dir.join("search.redb"), phrase) {
            Ok(ix) => *slot = Some(Arc::new(ix)),
            Err(e) => log(e),
        }
    }

    /// Closes the index for every holder (stop of the chat store: lock, account switch, drop).
    pub fn close(&self) {
        if let Some(ix) = self.slot.lock().take() {
            ix.close();
        }
    }

    /// The open index, if any.
    pub fn index(&self) -> Option<Arc<SearchIndex>> {
        self.slot.lock().clone().filter(|s| !s.is_closed())
    }

    /// Test switch: skip the next `n` hook writes.
    pub fn skip_next_writes(&self, n: u32) {
        self.skip.store(n, Ordering::SeqCst);
    }

    fn skipped(&self) -> bool {
        self.skip.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1)).is_ok()
    }

    // ---- hooks -----------------------------------------------------------------------------

    /// After a committed 1:1 change: index the touched messages; the shard's watermark moves
    /// from `prev` to `vv` only if it was at `prev`.
    pub fn dm(&self, did: &str, pair: &str, day: &str, recs: &[&MsgRec], prev: Option<Vec<u8>>, vv: Vec<u8>) {
        if self.skipped() {
            return;
        }
        let Some(ix) = self.index() else { return };
        let rows: Vec<DocRow> = recs.iter().map(|r| dm_row(did, pair, day, r)).collect();
        let mark = Mark { src: dm_src(pair), day: day.to_string(), prev, vv };
        if let Err(e) = ix.apply(&rows, &[], &[mark]) {
            log(e);
        }
    }

    /// After a group change: index the posts that show now, per day doc; each day's watermark
    /// moves from `prev` to `vv` only if it was at `prev`.
    pub fn group(&self, gid: &str, diffs: &[PostDiff]) {
        if diffs.is_empty() || self.skipped() {
            return;
        }
        let Some(ix) = self.index() else { return };
        let src = group_src(gid);
        let rows: Vec<DocRow> = diffs.iter().flat_map(|d| d.put.iter().map(|i| group_row(gid, &d.day, i))).collect();
        let marks: Vec<Mark> = diffs.iter().map(|d| Mark { src: src.clone(), day: d.day.clone(), prev: Some(d.prev.clone()), vv: d.vv.clone() }).collect();
        if let Err(e) = ix.apply(&rows, &[], &marks) {
            log(e);
        }
    }

    /// A conversation went from this device (a forgotten group): its docs leave the index.
    pub fn forget(&self, src: &str) {
        if let Some(ix) = self.index()
            && let Err(e) = ix.forget(src)
        {
            log(e);
        }
    }
}
