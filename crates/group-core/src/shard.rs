//! Day docs in memory (`docs/group-shards.md`): a bounded cache of the day docs of one group,
//! each with its posts judged against the group's meta state (`state::post_visible`).
//!
//! Nothing here runs over a group's whole history: a day is loaded when something touches it
//! (a write, an arriving update, a page, a search reconcile), judged as it loads, and dropped
//! again when more than `MAX_SHARDS_CACHED` are held. Loading reads our own sealed rows and does
//! not verify signatures again (verify once, on arrival).

use std::collections::HashMap;

use super::doc::{DocId, Entry, GroupDoc};
use super::state::{GroupState, Item, Vis, post_item, post_visible};
use super::store::{GroupMeta, GroupStore, PostBrief, ShardMeta};
use crate::Error;
use crdt::{DAY_MS, day_of};

/// Day docs of one group held in memory at most.
pub const MAX_SHARDS_CACHED: usize = 12;
/// A day this old (or older) is compacted into one snapshot row, as in 1:1 chat.
pub const CLOSE_AFTER_DAYS: i64 = 8;

/// One post held, with the verdict `post_visible` gave it.
pub struct PostRow {
    pub entry: Entry,
    pub vis: Vis,
}

/// One day doc and its posts.
pub struct Shard {
    pub doc: GroupDoc,
    pub meta: ShardMeta,
    /// Every post held, ordered by `(at, id)`.
    pub rows: Vec<PostRow>,
    used: u64,
}

fn key(e: &Entry) -> (i64, &str) {
    (e.rec.at, e.id.as_str())
}

impl Shard {
    /// Reads day `day` of `gid` from the store (an empty doc if we hold nothing of it) and judges
    /// its posts against `st`.
    pub fn load(store: &GroupStore, gid: &str, wid: u64, day: &str, st: &GroupState) -> Result<Shard, Error> {
        let (snap, ups) = store.shard_rows(gid, day)?;
        let meta = store.shard_meta(gid, day)?.unwrap_or_default();
        let id = DocId::Day(day.to_string());
        let doc = GroupDoc::load_in(gid, id, wid, snap.as_deref(), &ups).map_err(|e| Error::Io(format!("group {gid} day {day}: {e}")))?;
        let entries = doc.entries_trusted().map_err(|e| Error::Io(format!("group {gid} day {day}: {e}")))?;
        let mut s = Shard { doc, meta, rows: Vec::with_capacity(entries.len()), used: 0 };
        for e in entries {
            let vis = post_visible(st, &e.rec.author, &e.rec.seen);
            s.rows.push(PostRow { entry: e, vis });
        }
        s.rows.sort_by(|a, b| key(&a.entry).cmp(&key(&b.entry)));
        Ok(s)
    }

    pub fn has(&self, e: &Entry) -> bool {
        self.rows.binary_search_by(|r| key(&r.entry).cmp(&key(e))).is_ok()
    }

    /// Adds a post that just entered the doc.
    pub fn insert(&mut self, entry: Entry, vis: Vis) {
        let at = self.rows.partition_point(|r| key(&r.entry) < key(&entry));
        self.rows.insert(at, PostRow { entry, vis });
    }

    pub fn has_pending(&self) -> bool {
        self.rows.iter().any(|r| r.vis == Vis::Pending)
    }

    /// Judges the pending posts again (the meta doc advanced); returns those that show now.
    pub fn rejudge(&mut self, st: &GroupState) -> Vec<Item> {
        let mut shown = Vec::new();
        for r in self.rows.iter_mut().filter(|r| r.vis == Vis::Pending) {
            r.vis = post_visible(st, &r.entry.rec.author, &r.entry.rec.seen);
            if r.vis == Vis::Visible
                && let Some(i) = post_item(&r.entry, st)
            {
                shown.push(i);
            }
        }
        shown
    }

    /// The posts that show, oldest first.
    pub fn visible<'a>(&'a self, st: &'a GroupState) -> impl Iterator<Item = Item> + 'a {
        self.rows.iter().filter(|r| r.vis == Vis::Visible).filter_map(|r| post_item(&r.entry, st))
    }
}

/// The day docs of one group that are in memory.
#[derive(Default)]
pub struct Shards {
    map: HashMap<String, Shard>,
    tick: u64,
}

impl Shards {
    /// The day doc `day`, loaded if it is not cached (the least recently used one goes when the
    /// cache is full).
    pub fn get(&mut self, store: &GroupStore, gid: &str, wid: u64, day: &str, st: &GroupState) -> Result<&mut Shard, Error> {
        self.tick += 1;
        if !self.map.contains_key(day) {
            let s = Shard::load(store, gid, wid, day, st)?;
            if self.map.len() >= MAX_SHARDS_CACHED
                && let Some(old) = self.map.iter().min_by_key(|(_, s)| s.used).map(|(d, _)| d.clone())
            {
                self.map.remove(&old);
            }
            self.map.insert(day.to_string(), s);
        }
        let s = self.map.get_mut(day).expect("just inserted");
        s.used = self.tick;
        Ok(s)
    }

    pub fn remove(&mut self, day: &str) {
        self.map.remove(day);
    }

    /// A day doc if it is cached (no loading).
    pub fn peek(&self, day: &str) -> Option<&Shard> {
        self.map.get(day)
    }

    pub fn peek_mut(&mut self, day: &str) -> Option<&mut Shard> {
        self.map.get_mut(day)
    }

    pub fn cached(&self) -> usize {
        self.map.len()
    }
}

/// What a chat-list row keeps of a post.
pub fn brief(i: &Item) -> PostBrief {
    PostBrief { id: i.id.clone(), author: i.author.clone(), at: i.at, text: i.text.clone(), reply_to: i.reply_to.clone(), image: i.image.clone(), peer: i.peer, counter: i.counter }
}

pub fn brief_item(b: &PostBrief, st: &GroupState) -> Item {
    Item {
        id: b.id.clone(),
        kind: super::state::ItemKind::Post,
        author: b.author.clone(),
        subject: b.author.clone(),
        at: b.at,
        text: b.text.clone(),
        reply_to: b.reply_to.clone(),
        image: b.image.clone(),
        before_removal: st.removed.contains_key(&b.author),
        peer: b.peer,
        counter: b.counter,
    }
}

/// Books posts that show now in the group's local state: the chat-list row (newest), the unread
/// count (`me` is our DID) and the newest day.
pub fn note_posts(meta: &mut GroupMeta, me: &str, shown: &[Item]) {
    for i in shown {
        if meta.last.as_ref().is_none_or(|l| (l.at, &l.id) < (i.at, &i.id)) {
            meta.last = Some(brief(i));
        }
        if i.author != me && i.at > meta.read_upto {
            meta.unread += 1;
        }
    }
}

/// The calendar days (UTC, newest first) from today back `n` days.
pub fn recent_days(now_ms: i64, n: i64) -> Vec<String> {
    (0..n).map(|i| day_of(now_ms - i * DAY_MS)).collect()
}
