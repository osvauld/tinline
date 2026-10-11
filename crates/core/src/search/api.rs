//! The UniFFI surface of search and its runtime: the per-account index opened and closed with
//! the chat store, the hooks the chat and group engines call after a committed change, the
//! background reconcile, and the query (`docs/search.md`).

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use super::index::{DocRow, Mark, SearchIndex, vv_eq};
use super::text;
use crate::Error;
use crate::chat::doc::{MsgRec, pair_id};
use crate::group::engine::{GroupCore, PostDiff};
use crate::group::state::{Item, ItemKind};
use crate::node::{Inner, Node};

/// What a name hit opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SearchNameKind {
    /// `id` is the contact's DID (`chat_day`).
    Contact,
    /// `id` is the group id (`group_messages`).
    Group,
    /// The "You" conversation: `id` is our own DID.
    Me,
}

/// Where a message hit lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SearchConvKind {
    Dm,
    Group,
}

/// A highlighted stretch of a snippet: `start..end` in chars (Unicode scalar values) of the
/// snippet. Android converts to UTF-16 offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct HitRange {
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct NameHit {
    pub kind: SearchNameKind,
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MessageHit {
    pub kind: SearchConvKind,
    /// The contact's DID (our own for "You"), or the group id.
    pub conv: String,
    /// The contact's or group's name as shown in the lists ("You" for our own).
    pub conv_name: String,
    /// DMs: the UTC day to open with `chat_day(conv, day)`. Groups: `-` (see `docs/search.md`
    /// "Apps" for how to locate the post).
    pub day: String,
    pub id: String,
    /// Unix ms.
    pub at: u64,
    /// The author's DID.
    pub author: String,
    pub author_name: String,
    /// About 80 chars of the text around the first match; "…" marks a cut.
    pub snippet: String,
    pub highlights: Vec<HitRange>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SearchResults {
    /// People, groups and "You" whose name contains the query (first page only).
    pub names: Vec<NameHit>,
    /// Newest first.
    pub messages: Vec<MessageHit>,
    /// Pass back as `cursor` for the next page of messages; `None` at the end.
    pub next: Option<String>,
}

const MAX_LIMIT: u32 = 100;
/// Postings read per index page while filling a result page.
const SCAN: usize = 256;
/// Postings looked at per query at most (a very common prefix must not scan everything).
const SCAN_BUDGET: usize = 20_000;

#[uniffi::export]
impl Node {
    /// Local search over the selected account's 1:1 chats, groups and names. Every word of
    /// `query` must match the start of a word in the message (CJK/Thai-like scripts: anywhere in
    /// a run); case and Latin accents are ignored. `limit` messages per page (1-100); `cursor` =
    /// `next` of the previous page. Names come with the first page only. Fails `Locked` while
    /// locked.
    pub fn search(&self, query: String, limit: u32, cursor: Option<String>) -> Result<SearchResults, Error> {
        self.inner.search(&query, limit, cursor.as_deref(), SCAN_BUDGET)
    }
}

/// For tests only.
impl Node {
    /// `search` with another per-call posting budget.
    #[doc(hidden)]
    pub fn search_with_budget_for_test(&self, query: String, limit: u32, cursor: Option<String>, budget: u32) -> Result<SearchResults, Error> {
        self.inner.search(&query, limit, cursor.as_deref(), budget as usize)
    }

    /// The next `n` index writes of the chat and group hooks are skipped, as if the process died
    /// between the store commit and the index write.
    #[doc(hidden)]
    pub fn search_skip_next_writes_for_test(&self, n: u32) {
        self.inner.search_skip.store(n, std::sync::atomic::Ordering::SeqCst);
    }

    /// Puts a stale post row (as an older version of the post, or a post no longer shown) into
    /// the index of a group and forgets the group's watermark, so the next reconcile has to fix it.
    #[doc(hidden)]
    pub fn search_plant_group_post_for_test(&self, group_id: String, post_id: String, text: String) -> Result<(), Error> {
        let me = self.inner.me()?;
        let ix = self.inner.search_ix().ok_or(Error::Locked)?;
        let row = DocRow { src: group_src(&group_id), conv: group_id.clone(), day: "-".into(), id: post_id, at: 1, author: me.id.did().to_string(), text };
        ix.apply(&[row], &[], &[Mark { src: group_src(&group_id), day: "-".into(), prev: None, vv: Vec::new() }])
    }

    /// Compacts the old days now (otherwise every hour); returns how many days are closed.
    #[doc(hidden)]
    pub fn compact_now_for_test(&self) -> Result<u32, Error> {
        let inner = self.inner.clone();
        self.block_on(async move { inner.chat_compact_for_test().await })?
    }
}

/// Test switch (`synth.rs`): no reconcile in the background on open.
pub(crate) static NO_BG_RECONCILE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn dm_src(pair: &str) -> String {
    format!("dm.{pair}")
}

pub(crate) fn group_src(gid: &str) -> String {
    format!("g.{gid}")
}

/// What a 1:1 message contributes to the index: its text and the name of a file it carries.
fn dm_row(did: &str, pair: &str, day: &str, rec: &MsgRec) -> DocRow {
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

fn group_row(gid: &str, i: &Item) -> DocRow {
    DocRow { src: group_src(gid), conv: gid.to_string(), day: "-".into(), id: i.id.clone(), at: i.at, author: i.author.clone(), text: i.text.clone() }
}

fn posts(items: &[Item]) -> impl Iterator<Item = &Item> {
    items.iter().filter(|i| i.kind == ItemKind::Post)
}

impl Inner {
    // ---- lifecycle -------------------------------------------------------------------------

    /// Opens the account's index next to `chat.redb` (called by `chat_open`). A failure leaves
    /// search off for this session; nothing else depends on it.
    pub(crate) fn search_open(&self, dir: &Path, phrase: &str) {
        let mut slot = self.search.lock();
        if slot.as_ref().is_some_and(|s| !s.is_closed()) {
            return;
        }
        match SearchIndex::open(&dir.join("search.redb"), phrase) {
            Ok(ix) => *slot = Some(Arc::new(ix)),
            Err(e) => tracing::warn!("search index: {e}"),
        }
    }

    /// Closes the index for every holder (stop of the chat store: lock, account switch, drop).
    pub(crate) fn search_close(&self) {
        if let Some(ix) = self.search.lock().take() {
            ix.close();
        }
    }

    fn search_ix(&self) -> Option<Arc<SearchIndex>> {
        self.search.lock().clone().filter(|s| !s.is_closed())
    }

    /// Test switch: this hook write is to be skipped.
    fn search_skipped(&self) -> bool {
        use std::sync::atomic::Ordering;
        self.search_skip.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1)).is_ok()
    }

    fn search_log(e: Error) {
        tracing::warn!("search index: {e}");
    }

    // ---- hooks -----------------------------------------------------------------------------

    /// After a committed 1:1 change: index the touched messages; the shard's watermark moves
    /// from `prev` to `vv` only if it was at `prev`.
    pub(crate) fn search_dm(&self, did: &str, pair: &str, day: &str, recs: &[&MsgRec], prev: Option<Vec<u8>>, vv: Vec<u8>) {
        if self.search_skipped() {
            return;
        }
        let Some(ix) = self.search_ix() else { return };
        let rows: Vec<DocRow> = recs.iter().map(|r| dm_row(did, pair, day, r)).collect();
        let mark = Mark { src: dm_src(pair), day: day.to_string(), prev, vv };
        if let Err(e) = ix.apply(&rows, &[], &[mark]) {
            Self::search_log(e);
        }
    }

    /// After a group change (`group_after`): index new and changed posts, drop hidden ones.
    pub(crate) fn search_group(&self, gid: &str, d: &PostDiff) {
        if self.search_skipped() {
            return;
        }
        let Some(ix) = self.search_ix() else { return };
        let src = group_src(gid);
        let rows: Vec<DocRow> = d.put.iter().map(|i| group_row(gid, i)).collect();
        let gone: Vec<String> = d.gone.iter().map(|id| ix.doc_key(&src, id)).collect();
        let mark = Mark { src, day: "-".into(), prev: Some(d.prev.clone()), vv: d.vv.clone() };
        if let Err(e) = ix.apply(&rows, &gone, &[mark]) {
            Self::search_log(e);
        }
    }

    /// A conversation went from this device (a forgotten group): its docs leave the index.
    pub(crate) fn search_forget(&self, src: &str) {
        if let Some(ix) = self.search_ix()
            && let Err(e) = ix.forget(src)
        {
            Self::search_log(e);
        }
    }

    // ---- reconcile -------------------------------------------------------------------------

    /// In the background after open: every shard and group whose version differs from its
    /// watermark is indexed again (all of them after a rebuild). Never blocks the unlock.
    pub(crate) fn search_reconcile(self: &Arc<Self>) {
        if NO_BG_RECONCILE.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let Some(ix) = self.search_ix() else { return };
        let this = self.clone();
        self.handle.spawn(async move {
            if let Err(e) = this.search_reconcile_dms(&ix).await {
                tracing::debug!("search reconcile: {e}");
            }
            let gc = this.group.core.lock().clone();
            if let Some(gc) = gc.filter(|c| !c.closed()) {
                this.search_reconcile_groups(&gc);
            }
        });
    }

    async fn search_reconcile_dms(self: &Arc<Self>, ix: &Arc<SearchIndex>) -> Result<(), Error> {
        let core = self.chat_core()?;
        let me = self.me()?;
        let mut owners: Vec<String> = {
            let s = self.shared.lock();
            s.state.contacts.iter().map(|c| c.did.clone()).chain(s.state.notices.keys().cloned()).collect()
        };
        owners.push(me.id.did().to_string());
        let mut seen = HashSet::new();
        for did in owners {
            let pair = pair_id(me.id.did(), &did);
            if !seen.insert(pair.clone()) {
                continue;
            }
            let src = dm_src(&pair);
            for day in core.store.shard_days(&pair)? {
                if ix.is_closed() || core.store.db.raw().is_closed() {
                    return Ok(());
                }
                let Some(meta) = core.store.shard_meta(&pair, &day)? else { continue };
                if ix.watermark(&src, &day).is_some_and(|w| vv_eq(&w, &meta.vv)) {
                    continue;
                }
                // A closed day whose snapshot blob is not here (yet) is skipped: next time.
                let shard = match self.chat_load_shard(&core, &pair, &day).await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::debug!("search reconcile {day}: {e}");
                        continue;
                    }
                };
                let msgs = shard.messages().map_err(|e| Error::Io(e.to_string()))?;
                let rows: Vec<DocRow> = msgs.values().map(|r| dm_row(&did, &pair, &day, r)).collect();
                let mark = Mark { src: src.clone(), day: day.clone(), prev: None, vv: shard.vv().encode() };
                let ix2 = ix.clone();
                tokio::task::spawn_blocking(move || ix2.apply(&rows, &[], &[mark])).await.map_err(|e| Error::Io(e.to_string()))??;
            }
        }
        Ok(())
    }

    /// Groups are held in memory whole: each whose watermark differs is diffed against the index
    /// (new and changed posts written, posts no longer shown dropped). In the background.
    pub(crate) fn search_reconcile_groups(self: &Arc<Self>, core: &Arc<GroupCore>) {
        if NO_BG_RECONCILE.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        drop(self.search_reconcile_groups_run(core));
    }

    /// For the year-scale measurements: both reconcile parts now, timed.
    pub(crate) async fn search_reconcile_now(self: &Arc<Self>) -> Result<(std::time::Duration, std::time::Duration), Error> {
        let ix = self.search_ix().ok_or(Error::Locked)?;
        let t = std::time::Instant::now();
        self.search_reconcile_dms(&ix).await?;
        let dms = t.elapsed();
        let t = std::time::Instant::now();
        let gc = self.group_core()?;
        let _ = self.search_reconcile_groups_run(&gc).await;
        Ok((dms, t.elapsed()))
    }

    fn search_reconcile_groups_run(self: &Arc<Self>, core: &Arc<GroupCore>) -> n0_future::task::JoinHandle<()> {
        let Some(ix) = self.search_ix() else { return self.handle.spawn_blocking(|| {}) };
        let core = core.clone();
        self.handle.spawn_blocking(move || {
            let gids: Vec<String> = core.groups.lock().keys().cloned().collect();
            for gid in gids {
                if ix.is_closed() || core.closed() {
                    return;
                }
                let src = group_src(&gid);
                let Some((items, vv)) = core.groups.lock().get(&gid).map(|l| (l.state.items.clone(), l.doc.vv().encode())) else { continue };
                if ix.watermark(&src, "-").is_some_and(|w| vv_eq(&w, &vv)) {
                    continue;
                }
                let rows: Vec<DocRow> = posts(&items).map(|i| group_row(&gid, i)).collect();
                let live: HashSet<String> = rows.iter().map(|r| ix.doc_key(&src, &r.id)).collect();
                let stale: Vec<String> = match ix.doc_keys(&src) {
                    Ok(k) => k.into_iter().filter(|h| !live.contains(h)).collect(),
                    Err(e) => return Self::search_log(e),
                };
                if let Err(e) = ix.apply(&rows, &stale, &[Mark { src, day: "-".into(), prev: None, vv }]) {
                    return Self::search_log(e);
                }
            }
        })
    }

    // ---- query -----------------------------------------------------------------------------

    pub(crate) fn search(self: &Arc<Self>, query: &str, limit: u32, cursor: Option<&str>, budget: usize) -> Result<SearchResults, Error> {
        let me = self.me()?;
        self.chat_core()?;
        let ix = self.search_ix().ok_or(Error::Locked)?;
        let gcore = self.group_core().ok();
        let names = if cursor.is_none() { self.search_names(&me, gcore.as_deref(), query) } else { Vec::new() };
        let terms = text::query_terms(query);
        let Some(scan) = terms.iter().max_by_key(|t| t.weight()) else {
            return Ok(SearchResults { names, messages: Vec::new(), next: None });
        };
        if let Some(c) = cursor
            && !c.split_once('/').is_some_and(|(r, h)| r.len() == 16 && h.len() == 16 && c.bytes().all(|b| b.is_ascii_hexdigit() || b == b'/'))
        {
            return Err(Error::Protocol("bad cursor".into()));
        }
        let limit = limit.clamp(1, MAX_LIMIT) as usize;
        let lookup = scan.lookup();
        let readable = self.search_readable(&me);
        let mut messages = Vec::new();
        let mut after = cursor.map(str::to_string);
        let mut looked = 0;
        let mut next = None;
        'scan: loop {
            let (page, more) = ix.page(&lookup, after.as_deref(), SCAN)?;
            for (cur, row) in page {
                looked += 1;
                after = Some(cur.clone());
                let Some(row) = row else { continue };
                if let Some(hit) = self.search_hit(&me, gcore.as_deref(), &readable, &terms, row) {
                    messages.push(hit);
                    if messages.len() >= limit {
                        next = Some(cur);
                        break 'scan;
                    }
                }
                if looked >= budget {
                    next = Some(cur);
                    break 'scan;
                }
            }
            if !more {
                break;
            }
        }
        Ok(SearchResults { names, messages, next })
    }

    /// The 1:1 conversations the chat list shows (contacts, kept histories, "You").
    fn search_readable(&self, me: &crate::node::Me) -> HashSet<String> {
        let s = self.shared.lock();
        let mut out: HashSet<String> = s.state.contacts.iter().map(|c| c.did.clone()).chain(s.state.notices.keys().cloned()).collect();
        out.insert(me.id.did().to_string());
        out
    }

    fn search_hit(&self, me: &crate::node::Me, gcore: Option<&GroupCore>, readable: &HashSet<String>, terms: &[text::Term], row: DocRow) -> Option<MessageHit> {
        let group = row.src.starts_with("g.");
        let gc = if group { Some(gcore?) } else { None };
        let conv_name = match gc {
            Some(gc) => gc.groups.lock().get(&row.conv)?.state.name.clone(),
            None if !readable.contains(&row.conv) => return None,
            None if row.conv == me.id.did() => "You".to_string(),
            None => crate::block::known_name(&self.shared.lock().state, &row.conv).unwrap_or_default(),
        };
        let n = text::normalize(&row.text);
        let hits = text::match_all(terms, &n)?;
        let author_name = match gc {
            Some(gc) => self.group_name_of(gc, &row.conv, &row.author),
            None if row.author == me.id.did() => me.profile.name.clone(),
            None => crate::block::known_name(&self.shared.lock().state, &row.author).unwrap_or_default(),
        };
        let (snippet, hl) = text::snippet(&row.text, &n, &hits);
        Some(MessageHit {
            kind: if group { SearchConvKind::Group } else { SearchConvKind::Dm },
            conv: row.conv,
            conv_name,
            day: row.day,
            id: row.id,
            at: row.at.max(0) as u64,
            author: row.author,
            author_name,
            snippet,
            highlights: hl.into_iter().map(|(start, end)| HitRange { start, end }).collect(),
        })
    }

    /// Names live in memory (contacts, kept histories, groups): matched there, not indexed.
    fn search_names(&self, me: &crate::node::Me, gcore: Option<&GroupCore>, query: &str) -> Vec<NameHit> {
        let q = text::folded(query);
        if q.is_empty() {
            return Vec::new();
        }
        let hit = |name: &str| text::folded(name).contains(&q);
        let mut out = Vec::new();
        if hit("You") || hit(&me.profile.name) {
            out.push(NameHit { kind: SearchNameKind::Me, id: me.id.did().to_string(), name: "You".into() });
        }
        {
            let s = self.shared.lock();
            let mut people: Vec<(String, String)> = s.state.contacts.iter().map(|c| (c.did.clone(), crate::node::display_name(c))).collect();
            people.extend(s.state.notices.iter().filter(|(d, _)| !s.state.contacts.iter().any(|c| &c.did == *d)).map(|(d, h)| (d.clone(), h.name.clone())));
            people.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
            out.extend(people.into_iter().filter(|(_, n)| hit(n)).map(|(id, name)| NameHit { kind: SearchNameKind::Contact, id, name }));
        }
        if let Some(gc) = gcore {
            let mut groups: Vec<(String, String)> = gc.groups.lock().iter().filter(|(_, l)| !l.state.name.is_empty()).map(|(g, l)| (g.clone(), l.state.name.clone())).collect();
            groups.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
            out.extend(groups.into_iter().filter(|(_, n)| hit(n)).map(|(id, name)| NameHit { kind: SearchNameKind::Group, id, name }));
        }
        out
    }
}
