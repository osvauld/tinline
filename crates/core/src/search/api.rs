//! The UniFFI surface of search and its runtime: the per-account index opened and closed with
//! the chat store, the hooks the chat and group engines call after a committed change, the
//! background reconcile, and the query (`docs/search.md`).

use std::collections::HashSet;
use std::sync::Arc;

use super::find;
use super::index::{DocRow, Mark, Mode, SearchIndex};
use super::text;
use crate::Error;
use crate::chat::doc::pair_id;
use crate::group::engine::GroupCore;
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

    /// Find in chat: the same search inside one conversation (`conv` is the contact's DID, our
    /// own for "You", or the group id). Newest first, same cursor, snippets and errors as
    /// `search`; `names` is empty. A conversation the chat list does not show, or a group this
    /// device does not hold, has no hits.
    pub fn search_in(&self, kind: SearchConvKind, conv: String, query: String, limit: u32, cursor: Option<String>) -> Result<SearchResults, Error> {
        self.inner.search_in(kind, &conv, &query, limit, cursor.as_deref(), SCAN_BUDGET, find::DROP_BUDGET, &mut FindStats::default())
    }
}

/// What one find-in-chat call did (tests).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FindStats {
    pub segs_opened: usize,
    pub dropped: usize,
    pub z_reads: usize,
}

/// For tests only.
impl Node {
    /// `search` with another per-call posting budget.
    #[doc(hidden)]
    pub fn search_with_budget_for_test(&self, query: String, limit: u32, cursor: Option<String>, budget: u32) -> Result<SearchResults, Error> {
        self.inner.search(&query, limit, cursor.as_deref(), budget as usize)
    }

    /// `search_in` with other budgets (this conversation's postings, other conversations'
    /// dropped ones); also returns (segments opened, postings dropped by the doc table).
    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    pub fn search_in_with_budget_for_test(&self, kind: SearchConvKind, conv: String, query: String, limit: u32, cursor: Option<String>, budget: u32, drop_budget: u32) -> Result<(SearchResults, u32, u32), Error> {
        let mut st = FindStats::default();
        let r = self.inner.search_in(kind, &conv, &query, limit, cursor.as_deref(), budget as usize, drop_budget as usize, &mut st)?;
        Ok((r, st.segs_opened as u32, st.dropped as u32))
    }

    /// The next `n` index writes of the chat and group hooks are skipped, as if the process died
    /// between the store commit and the index write.
    #[doc(hidden)]
    pub fn search_skip_next_writes_for_test(&self, n: u32) {
        self.inner.search.skip_next_writes(n);
    }

    /// Puts a stale post row (as an older version of the post, or a post no longer shown) into
    /// the index of a group under today's day and forgets that day's watermark, so the next
    /// reconcile has to fix it.
    #[doc(hidden)]
    pub fn search_plant_group_post_for_test(&self, group_id: String, post_id: String, text: String) -> Result<(), Error> {
        let me = self.inner.me()?;
        let ix = self.inner.search_ix().ok_or(Error::Locked)?;
        let day = crate::chat::doc::day_of(crate::node::now_ms() as i64);
        let row = DocRow { src: group_src(&group_id), conv: group_id.clone(), day: day.clone(), id: post_id, at: 1, author: me.id.did().to_string(), text };
        Ok(ix.apply(&[row], &[], &[Mark { src: group_src(&group_id), day, prev: None, vv: Vec::new() }])?)
    }

    /// Pretends today is `day` (`YYYY-MM-DD`) for the search index (`None`: the real clock).
    #[doc(hidden)]
    pub fn search_set_today_for_test(&self, day: Option<String>) -> Result<(), Error> {
        self.inner.search_ix().ok_or(Error::Locked)?.set_today(day.as_deref());
        Ok(())
    }

    /// Shrinks the hot-tier and segment caps of the search index.
    #[doc(hidden)]
    pub fn search_set_caps_for_test(&self, hot_max_docs: u32, seg_max_docs: u32) -> Result<(), Error> {
        self.inner.search_ix().ok_or(Error::Locked)?.set_limits(|l| {
            l.hot_max_docs = hot_max_docs as usize;
            l.seg_max_docs = seg_max_docs as usize;
        });
        Ok(())
    }

    /// Writes every cold day into segments now; returns (docs in memory, segments, docs in
    /// segments).
    #[doc(hidden)]
    pub fn search_maintain_for_test(&self) -> Result<(u32, u32, u32), Error> {
        let ix = self.inner.search_ix().ok_or(Error::Locked)?;
        ix.maintain(Mode::Force)?;
        let s = ix.stats();
        Ok((s.hot_docs as u32, s.segs as u32, s.seg_docs as u32))
    }

    /// Compacts the old days now (otherwise every hour); returns how many days are closed.
    #[doc(hidden)]
    pub fn compact_now_for_test(&self) -> Result<u32, Error> {
        self.run(|inner| async move { inner.chat.chat_compact_for_test().await })
    }
}

pub(crate) use search_ix::NO_BG_RECONCILE;

pub(crate) use search_ix::{dm_row, dm_src, group_src};

/// A cursor is `{rev_at}/{h}`, both 16 hex digits.
fn check_cursor(cursor: Option<&str>) -> Result<(), Error> {
    if let Some(c) = cursor
        && !c.split_once('/').is_some_and(|(r, h)| r.len() == 16 && h.len() == 16 && c.bytes().all(|b| b.is_ascii_hexdigit() || b == b'/'))
    {
        return Err(Error::Protocol("bad cursor".into()));
    }
    Ok(())
}

/// The heaviest term first (it drives the scan), then every term.
fn scan_lookups(scan: &text::Term, terms: &[text::Term]) -> Vec<String> {
    let mut lookups: Vec<String> = vec![scan.lookup()];
    lookups.extend(terms.iter().map(|t| t.lookup()));
    lookups
}

impl Inner {
    fn search_ix(&self) -> Option<Arc<SearchIndex>> {
        self.search.index()
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
                let _ = this.group.search_reconcile_groups_run(&gc).await;
            }
            // Days that are cold now (and what the bulk left in memory) go into segments.
            let ix2 = ix.clone();
            let _ = tokio::task::spawn_blocking(move || ix2.maintain(Mode::Force)).await;
            if !ix.claim_ticker() {
                return;
            }
            // Then once a minute: days that fell out of the hot window or waited out the debounce.
            while !ix.is_closed() {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                let ix2 = ix.clone();
                if let Ok(Err(e)) = tokio::task::spawn_blocking(move || ix2.maintain(Mode::Tick)).await {
                    tracing::debug!("search maintain: {e}");
                }
            }
        });
    }

    async fn search_reconcile_dms(self: &Arc<Self>, ix: &Arc<SearchIndex>) -> Result<(), Error> {
        let core = self.chat.chat_core()?;
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
                if ix.watermark_is(&src, &day, &meta.vv) {
                    continue;
                }
                // A closed day whose snapshot blob is not here (yet) is skipped: next time.
                let shard = match self.chat.chat_load_shard(&core, &pair, &day).await {
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
                tokio::task::spawn_blocking(move || ix2.apply_bulk(&rows, &[], &[mark])).await.map_err(|e| Error::Io(e.to_string()))??;
            }
        }
        Ok(())
    }


    /// For the year-scale measurements: both reconcile parts now, timed.
    #[cfg(any(test, feature = "dev-tools"))]
    pub(crate) async fn search_reconcile_now(self: &Arc<Self>) -> Result<(std::time::Duration, std::time::Duration), Error> {
        let ix = self.search_ix().ok_or(Error::Locked)?;
        let t = std::time::Instant::now();
        self.search_reconcile_dms(&ix).await?;
        let dms = t.elapsed();
        let t = std::time::Instant::now();
        let gc = self.group.group_core()?;
        let _ = self.group.search_reconcile_groups_run(&gc).await;
        // The cold days the bulk left in memory go into segments (the open's last step; its
        // time counts in the groups part).
        let ix2 = ix.clone();
        tokio::task::spawn_blocking(move || ix2.maintain(Mode::Force)).await.map_err(|e| Error::Io(e.to_string()))??;
        Ok((dms, t.elapsed()))
    }

    // ---- query -----------------------------------------------------------------------------

    pub(crate) fn search(self: &Arc<Self>, query: &str, limit: u32, cursor: Option<&str>, budget: usize) -> Result<SearchResults, Error> {
        let me = self.me()?;
        self.chat.chat_core()?;
        let ix = self.search_ix().ok_or(Error::Locked)?;
        let gcore = self.group.group_core().ok();
        let names = if cursor.is_none() { self.search_names(&me, gcore.as_deref(), query) } else { Vec::new() };
        let terms = text::query_terms(query);
        let Some(scan) = terms.iter().max_by_key(|t| t.weight()) else {
            return Ok(SearchResults { names, messages: Vec::new(), next: None });
        };
        check_cursor(cursor)?;
        let limit = limit.clamp(1, MAX_LIMIT) as usize;
        let lookups = scan_lookups(scan, &terms);
        let readable = self.search_readable(&me);
        let mut messages = Vec::new();
        let mut scan = ix.scan_all(&lookups, cursor)?;
        let mut looked = 0;
        let mut next = None;
        'scan: loop {
            let (page, more) = scan.next(SCAN)?;
            for (cur, row) in page {
                looked += 1;
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

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn search_in(self: &Arc<Self>, kind: SearchConvKind, conv: &str, query: &str, limit: u32, cursor: Option<&str>, budget: usize, drop_budget: usize, stats: &mut FindStats) -> Result<SearchResults, Error> {
        let me = self.me()?;
        self.chat.chat_core()?;
        let ix = self.search_ix().ok_or(Error::Locked)?;
        let gcore = self.group.group_core().ok();
        let none = SearchResults { names: Vec::new(), messages: Vec::new(), next: None };
        let terms = text::query_terms(query);
        let Some(scan_term) = terms.iter().max_by_key(|t| t.weight()) else { return Ok(none) };
        check_cursor(cursor)?;
        let readable = self.search_readable(&me);
        let src = match kind {
            SearchConvKind::Dm if readable.contains(conv) => dm_src(&pair_id(me.id.did(), conv)),
            SearchConvKind::Group if gcore.as_ref().is_some_and(|g| g.groups.lock().contains_key(conv)) => group_src(conv),
            _ => return Ok(none),
        };
        let limit = limit.clamp(1, MAX_LIMIT) as usize;
        let lookups = scan_lookups(scan_term, &terms);
        let mut messages = Vec::new();
        let mut scan = ix.scan_in(&lookups, cursor, &src, drop_budget)?;
        let mut looked = 0;
        let next;
        'scan: loop {
            let (page, more) = scan.next(SCAN)?;
            for (cur, row) in page {
                looked += 1;
                if row.src == src
                    && let Some(hit) = self.search_hit(&me, gcore.as_deref(), &readable, &terms, row)
                {
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
                next = scan.stopped();
                break;
            }
        }
        (stats.segs_opened, stats.dropped, stats.z_reads) = scan.find_stats();
        Ok(SearchResults { names: Vec::new(), messages, next })
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
            Some(gc) => self.group.group_name_of(gc, &row.conv, &row.author),
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
            people.sort_by_key(|p| p.1.to_lowercase());
            out.extend(people.into_iter().filter(|(_, n)| hit(n)).map(|(id, name)| NameHit { kind: SearchNameKind::Contact, id, name }));
        }
        if let Some(gc) = gcore {
            let mut groups: Vec<(String, String)> = gc.groups.lock().iter().filter(|(_, l)| !l.state.name.is_empty()).map(|(g, l)| (g.clone(), l.state.name.clone())).collect();
            groups.sort_by_key(|g| g.1.to_lowercase());
            out.extend(groups.into_iter().filter(|(_, n)| hit(n)).map(|(id, name)| NameHit { kind: SearchNameKind::Group, id, name }));
        }
        out
    }
}
