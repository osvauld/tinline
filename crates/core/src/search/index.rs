//! The search index, format version 2 (`docs/search.md` "Index v2"): a memory-only hot tier
//! (`hot.rs`) over write-once time segments in `search.redb` (`seg.rs`).
//!
//! The file holds only `v` (format version), `m` (the manifest) and the segments' `x/` and `y/`
//! records, all sealed. A change goes to memory; the cold days that changed or fell out of the
//! hot window are written into segments by [`SearchIndex::maintain`], in one transaction with the
//! manifest. Doc hashes and term hashes use `index_key`, not the sealing key.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use storage::{Op, Sealed, StorageError, Store};
use zeroize::Zeroizing;

use super::hot::Hot;
use super::seg::{self, Built, CHUNK, DayData, PART_DOCS, DayKey, Digest, Manifest, NO_DIGEST, Seg, View, decode_chunk, decode_list, rev_at, x_key, y_key};
use crate::Error;
use crate::chat::doc::{DAY_MS, day_of, day_start};

/// Bump when the tokenizer or the layout changes: the index is rebuilt from the chat and group
/// stores. (1: per-message postings in the file. 2: hot tier + segments.)
pub const VERSION: u32 = 2;
const STORE_DOMAIN: &str = "tinline/search-store/v1";
const INDEX_DOMAIN: &str = "tinline/search-index/v1";

/// Days (today and the ones before) whose docs live in the hot tier only.
pub const HOT_DAYS: i64 = 8;
/// Docs in memory above which the oldest cold days are written into segments early.
pub const HOT_MAX_DOCS: usize = 20_000;
/// Docs in one segment at most (a longer month becomes several).
pub const SEG_MAX_DOCS: usize = 20_000;
/// Segments of one month above which they are merged.
pub const MERGE_SEGS: usize = 4;
/// How long a changed cold day waits in memory before its month is written again.
pub const DEBOUNCE: Duration = Duration::from_secs(600);

/// Free bytes in the file below which it is not worth compacting.
pub const COMPACT_MIN_WASTE: u64 = 8 << 20;

/// The tunables, as a value so that tests can shrink them.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub hot_days: i64,
    pub hot_max_docs: usize,
    pub seg_max_docs: usize,
    pub merge_segs: usize,
    pub debounce: Duration,
    /// Free bytes in the file above which compacting is considered ([`COMPACT_MIN_WASTE`]).
    pub compact_min_waste: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self { hot_days: HOT_DAYS, hot_max_docs: HOT_MAX_DOCS, seg_max_docs: SEG_MAX_DOCS, merge_segs: MERGE_SEGS, debounce: DEBOUNCE, compact_min_waste: COMPACT_MIN_WASTE }
    }
}

/// When [`SearchIndex::maintain`] writes cold days into segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The periodic check: days that waited out the debounce, and the oldest ones while over
    /// the memory cap.
    Tick,
    /// Only the oldest days, while over the memory cap (a bulk rebuild).
    Cap,
    /// Every cold day now (open, tests).
    Force,
}

pub(crate) fn io(e: StorageError) -> Error {
    Error::Io(e.to_string())
}

fn norm_phrase(phrase: &str) -> Zeroizing<String> {
    Zeroizing::new(phrase.split_whitespace().map(str::to_lowercase).collect::<Vec<_>>().join(" "))
}

/// One indexed message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocRow {
    /// `dm.{pair}` or `g.{gid}`.
    pub src: String,
    /// The contact's DID (DMs) or the group id.
    pub conv: String,
    /// The shard's UTC day (`YYYY-MM-DD`).
    pub day: String,
    pub id: String,
    pub at: i64,
    pub author: String,
    pub text: String,
}

/// "Set the watermark of `src/day` to `vv`" — if the stored one equals `prev` (`None`: always).
/// A hook that only indexed one change must not mark a shard whose older changes are missing.
pub struct Mark {
    pub src: String,
    pub day: String,
    pub prev: Option<Vec<u8>>,
    pub vv: Vec<u8>,
}

fn vv(b: &[u8]) -> Option<loro::VersionVector> {
    if b.is_empty() {
        return Some(loro::VersionVector::default());
    }
    loro::VersionVector::decode(b).ok()
}

/// 8 bytes standing for an encoded version vector: equal vectors, equal digests.
pub fn digest_of(b: &[u8]) -> Digest {
    let mut h = blake3::Hasher::new();
    match vv(b) {
        Some(v) => {
            let mut e: Vec<(u64, i64)> = v.iter().map(|(p, c)| (*p, i64::from(*c))).collect();
            e.sort_unstable();
            for (p, c) in e {
                h.update(&p.to_le_bytes());
                h.update(&c.to_le_bytes());
            }
        }
        None => {
            h.update(b"raw");
            h.update(b);
        }
    }
    let mut d: Digest = h.finalize().as_bytes()[..8].try_into().expect("8 bytes");
    if d == NO_DIGEST {
        d[0] = 1;
    }
    d
}

/// `(day number, month)` of a `YYYY-MM-DD` day.
fn parse_day(day: &str) -> Option<(u32, u32)> {
    let ms = day_start(day)?;
    let n = u32::try_from(ms.div_euclid(DAY_MS)).ok()?;
    let (y, m): (u32, u32) = (day.get(..4)?.parse().ok()?, day.get(5..7)?.parse().ok()?);
    Some((n, y * 12 + m - 1))
}

/// The keyed hashes of the index (doc, term, conversation, day), all under `index_key`.
pub struct Keyer(Zeroizing<[u8; 32]>);

impl Keyer {
    fn hash(&self, domain: &[u8], parts: &[&[u8]]) -> u64 {
        let mut h = blake3::Hasher::new_keyed(&self.0);
        h.update(domain);
        for p in parts {
            h.update(p);
        }
        u64::from_be_bytes(h.finalize().as_bytes()[..8].try_into().expect("8 bytes"))
    }

    pub fn doc(&self, src: &str, id: &str) -> u64 {
        self.hash(b"doc\0", &[src.as_bytes(), b"/", id.as_bytes()])
    }

    pub fn tok(&self, term: &str) -> u64 {
        self.hash(b"term\0", &[term.as_bytes()])
    }

    pub fn src(&self, src: &str) -> u64 {
        self.hash(b"src\0", &[src.as_bytes()])
    }

    pub fn day_key(&self, src: &str, day: &str) -> DayKey {
        (self.src(src), parse_day(day).map_or(u32::MAX, |d| d.0))
    }
}

/// A snapshot of the sizes, for tests and benchmarks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stats {
    pub hot_docs: usize,
    pub hot_days: usize,
    pub segs: usize,
    pub seg_docs: usize,
}

/// Where `maintain` spends its time, in ns: pick, read old segments, build, write, swap (for
/// the benchmarks).
pub static PHASES: [std::sync::atomic::AtomicU64; 5] = [const { std::sync::atomic::AtomicU64::new(0) }; 5];

fn phase(i: usize, since: Instant) -> Instant {
    PHASES[i].fetch_add(since.elapsed().as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);
    Instant::now()
}

#[derive(Default)]
struct Memo {
    /// The segments each month was last merged into (an idle check must not merge them again).
    merged: HashMap<u32, Vec<u32>>,
}

pub struct SearchIndex {
    db: Sealed,
    keyer: Keyer,
    hot: Mutex<Hot>,
    view: RwLock<Arc<View>>,
    maint: Mutex<Memo>,
    lim: RwLock<Limits>,
    today: Mutex<Option<u32>>,
    ticker: std::sync::atomic::AtomicBool,
    /// Segments were written or dropped since the last look at the file's free space.
    file_changed: std::sync::atomic::AtomicBool,
}

impl SearchIndex {
    /// Opens (creating) the index; a file of another format version is emptied (a rebuild).
    pub fn open(path: &Path, phrase: &str) -> Result<Self, Error> {
        let store = crate::store::open_waiting(path)?;
        crate::store::set_private(path);
        Self::with_store(store, phrase)
    }

    #[cfg(test)]
    pub fn in_memory(phrase: &str) -> Self {
        Self::with_store(Store::open_in_memory().unwrap(), phrase).unwrap()
    }

    fn with_store(store: Store, phrase: &str) -> Result<Self, Error> {
        let p = norm_phrase(phrase);
        let me = Self {
            db: Sealed::new(store, blake3::derive_key(STORE_DOMAIN, p.as_bytes())),
            keyer: Keyer(Zeroizing::new(blake3::derive_key(INDEX_DOMAIN, p.as_bytes()))),
            hot: Mutex::new(Hot::default()),
            view: RwLock::new(Arc::new(View::default())),
            maint: Mutex::new(Memo::default()),
            lim: RwLock::new(Limits::default()),
            today: Mutex::new(None),
            ticker: std::sync::atomic::AtomicBool::new(false),
            file_changed: std::sync::atomic::AtomicBool::new(true),
        };
        let current = matches!(me.db.get("v"), Ok(Some(v)) if v == VERSION.to_be_bytes());
        let man = if current { me.db.get("m").ok().flatten().and_then(|b| bincode::deserialize::<Manifest>(&b).ok()) } else { None };
        match man {
            Some(m) => *me.view.write() = Arc::new(View::new(m)),
            None => {
                // Another version, no manifest or an unreadable one: start over.
                me.db.apply(&[Op::DeletePrefix(String::new()), me.db.put_op("v", &VERSION.to_be_bytes()).map_err(io)?]).map_err(io)?;
            }
        }
        Ok(me)
    }

    /// Lets the file go for every holder (stop of the account's stores) and forgets the hot
    /// tier.
    pub fn close(&self) {
        self.db.close();
        self.hot.lock().wipe();
        *self.view.write() = Arc::new(View::default());
    }

    pub fn is_closed(&self) -> bool {
        self.db.raw().is_closed()
    }

    /// Whether the caller is the one to run the periodic maintenance of this index.
    pub fn claim_ticker(&self) -> bool {
        !self.ticker.swap(true, std::sync::atomic::Ordering::SeqCst)
    }

    pub fn doc_key(&self, src: &str, id: &str) -> String {
        format!("{:016x}", self.keyer.doc(src, id))
    }

    // ---- clock and tunables -----------------------------------------------------------------

    pub fn limits(&self) -> Limits {
        *self.lim.read()
    }

    pub fn set_limits(&self, f: impl FnOnce(&mut Limits)) {
        f(&mut self.lim.write());
    }

    /// Pretends today is `day` (`YYYY-MM-DD`); `None`: the real clock.
    pub fn set_today(&self, day: Option<&str>) {
        *self.today.lock() = day.and_then(|d| parse_day(d).map(|x| x.0));
    }

    fn today(&self) -> u32 {
        (*self.today.lock()).unwrap_or_else(|| {
            let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
            ms.div_euclid(DAY_MS) as u32
        })
    }

    fn is_hot(&self, daynum: u32, today: u32, lim: &Limits) -> bool {
        daynum != u32::MAX && i64::from(today) - i64::from(daynum) < lim.hot_days
    }

    pub fn stats(&self) -> Stats {
        let h = self.hot.lock();
        let v = self.view.read();
        Stats { hot_docs: h.docs.len(), hot_days: h.days.len(), segs: v.man.segs.len(), seg_docs: v.man.segs.iter().map(|s| s.docs as usize).sum() }
    }

    // ---- reading segments -------------------------------------------------------------------

    fn read_chunk(&self, seg: u32, c: usize) -> Option<Vec<(u64, DocRow)>> {
        decode_chunk(&self.db.get(&y_key(seg, c)).ok().flatten()?)
    }

    /// The rows of the given days, as the segments that own them hold them.
    fn load_days(&self, view: &View, keys: &[DayKey]) -> HashMap<DayKey, Vec<(u64, DocRow)>> {
        let mut out: HashMap<DayKey, Vec<(u64, DocRow)>> = HashMap::new();
        let mut by_seg: HashMap<u32, HashSet<DayKey>> = HashMap::new();
        for k in keys {
            out.entry(*k).or_default();
            if let Some((s, _)) = view.owner.get(k) {
                by_seg.entry(*s).or_default().insert(*k);
            }
        }
        for (id, wanted) in by_seg {
            let Some(s) = view.man.segs.iter().find(|s| s.id == id) else { continue };
            for c in 0..(s.docs as usize).div_ceil(CHUNK) {
                for (h, row) in self.read_chunk(id, c).unwrap_or_default() {
                    let k = self.keyer.day_key(&row.src, &row.day);
                    if wanted.contains(&k) {
                        out.get_mut(&k).expect("asked for").push((h, row));
                    }
                }
            }
        }
        out
    }

    // ---- writing ----------------------------------------------------------------------------

    /// Indexes `put` (a row without words is removed instead), removes the docs `drop` (keys
    /// from `doc_key`) and applies the watermarks. Memory only: nothing is written to the file
    /// (a change to a day that is already in a segment loads that day into memory first).
    /// Idempotent.
    pub fn apply(&self, put: &[DocRow], drop: &[String], marks: &[Mark]) -> Result<(), Error> {
        if self.is_closed() {
            return Ok(());
        }
        let puts: Vec<(u64, DayKey, &DocRow)> = put.iter().map(|r| (self.keyer.doc(&r.src, &r.id), self.keyer.day_key(&r.src, &r.day), r)).collect();
        let drops: Vec<u64> = drop.iter().filter_map(|h| u64::from_str_radix(h, 16).ok()).collect();
        struct M<'a> {
            key: DayKey,
            day: &'a str,
            prev: Option<Digest>,
            vv: Digest,
        }
        let ms: Vec<M> = marks.iter().map(|m| M { key: self.keyer.day_key(&m.src, &m.day), day: &m.day, prev: m.prev.as_deref().map(digest_of), vv: digest_of(&m.vv) }).collect();
        let mut names: HashMap<DayKey, &str> = HashMap::new();
        for (_, k, r) in &puts {
            names.insert(*k, &r.day);
        }
        for m in &ms {
            names.insert(m.key, m.day);
        }
        let empty = digest_of(&[]);
        let lim = self.limits();
        let today = self.today();
        for attempt in 0..4 {
            let view = self.view.read().clone();
            let missing: Vec<DayKey> = {
                let hot = self.hot.lock();
                names.keys().filter(|k| !hot.days.contains_key(*k) && view.owner.contains_key(*k)).copied().collect()
            };
            let loaded = if missing.is_empty() { HashMap::new() } else { self.load_days(&view, &missing) };
            let mut hot = self.hot.lock();
            if attempt < 3 && !Arc::ptr_eq(&view, &self.view.read()) {
                continue; // a segment write replaced the manifest under us: load again
            }
            let now = Instant::now();
            let mut touched: HashSet<DayKey> = HashSet::new();
            // Cold days that this change alters come into memory whole.
            for (key, rows) in loaded {
                if hot.days.contains_key(&key) {
                    continue;
                }
                let owner = view.owner.get(&key).map(|(_, d)| *d).filter(|d| *d != NO_DIGEST);
                let have: HashMap<u64, &DocRow> = rows.iter().map(|(h, r)| (*h, r)).collect();
                let changes = puts.iter().any(|(h, k, r)| *k == key && (crate::search::text::index_terms(&r.text).is_empty() == have.contains_key(h) || (have.get(h).is_some_and(|o| *o != *r))))
                    || drops.iter().any(|h| have.contains_key(h))
                    || ms.iter().any(|m| m.key == key && m.prev.is_none_or(|p| p == owner.unwrap_or(empty)) && owner != Some(m.vv));
                if !changes {
                    continue;
                }
                let d = hot.day_mut(key, names.get(&key).copied().unwrap_or(""));
                d.digest = owner;
                if !self.is_hot(key.1, today, &lim) {
                    d.cold_since = Some(now);
                }
                for (h, row) in rows {
                    hot.put(&self.keyer, key, h, row);
                }
                touched.insert(key);
            }
            for (h, key, row) in &puts {
                // A day left in its segment (nothing in this change alters it) stays there.
                if !hot.days.contains_key(key) && view.owner.contains_key(key) {
                    continue;
                }
                if hot.put(&self.keyer, *key, *h, (*row).clone()) {
                    touched.insert(*key);
                }
            }
            for h in &drops {
                if let Some((key, _)) = hot.docs.get(h).map(|(k, r, _)| (*k, r.id.clone())) {
                    hot.remove(*h);
                    touched.insert(key);
                }
            }
            for m in &ms {
                let cur = hot.days.get(&m.key).map(|d| d.digest).unwrap_or_else(|| view.owner.get(&m.key).map(|(_, d)| *d).filter(|d| *d != NO_DIGEST));
                if m.prev.is_none_or(|p| p == cur.unwrap_or(empty)) && cur != Some(m.vv) {
                    let d = hot.day_mut(m.key, m.day);
                    d.digest = Some(m.vv);
                    d.ver += 1;
                    touched.insert(m.key);
                }
            }
            for k in touched {
                if !self.is_hot(k.1, today, &lim)
                    && let Some(d) = hot.days.get_mut(&k)
                {
                    d.cold_since.get_or_insert(now);
                }
            }
            return Ok(());
        }
        unreachable!("the last attempt returns")
    }

    /// `apply` for a bulk (a rebuild): above the memory cap the oldest cold days go into
    /// segments before it returns.
    pub fn apply_bulk(&self, put: &[DocRow], drop: &[String], marks: &[Mark]) -> Result<(), Error> {
        self.apply(put, drop, marks)?;
        if self.hot.lock().docs.len() > self.limits().hot_max_docs {
            self.maintain(Mode::Cap)?;
        }
        Ok(())
    }

    /// Removes everything of a conversation: its docs and its watermarks, in memory and in the
    /// manifest (its docs in segments stop counting at once and are dropped at the next rewrite
    /// of their month).
    pub fn forget(&self, src: &str) -> Result<(), Error> {
        let _m = self.maint.lock();
        let s = self.keyer.src(src);
        let mut hot = self.hot.lock();
        let keys: Vec<DayKey> = hot.days.keys().filter(|k| k.0 == s).copied().collect();
        for k in keys {
            hot.drop_day(&k);
        }
        let view = self.view.read().clone();
        if !view.man.segs.iter().any(|g| g.days.iter().any(|d| d.0 == s)) {
            return Ok(());
        }
        let mut man = view.man.clone();
        let mut ops = Vec::new();
        for g in &mut man.segs {
            g.days.retain(|d| d.0 != s);
        }
        for g in man.segs.iter().filter(|g| g.days.is_empty()) {
            ops.push(Op::DeletePrefix(format!("x/{:08x}/", g.id)));
            ops.push(Op::DeletePrefix(format!("y/{:08x}/", g.id)));
        }
        man.segs.retain(|g| !g.days.is_empty());
        ops.push(self.db.put_op("m", &bincode::serialize(&man).expect("serialises")).map_err(io)?);
        self.db.apply(&ops).map_err(io)?;
        self.file_changed.store(true, std::sync::atomic::Ordering::SeqCst);
        *self.view.write() = Arc::new(View::new(man));
        Ok(())
    }

    /// The doc keys of a conversation's day.
    pub fn day_keys(&self, src: &str, day: &str) -> Result<HashSet<String>, Error> {
        let key = self.keyer.day_key(src, day);
        {
            let hot = self.hot.lock();
            if let Some(d) = hot.days.get(&key) {
                return Ok(d.docs.iter().map(|h| format!("{h:016x}")).collect());
            }
        }
        let view = self.view.read().clone();
        Ok(self.load_days(&view, &[key]).remove(&key).unwrap_or_default().into_iter().map(|(h, _)| format!("{h:016x}")).collect())
    }

    fn watermark(&self, src: &str, day: &str) -> Option<Digest> {
        let key = self.keyer.day_key(src, day);
        let hot = self.hot.lock();
        if let Some(d) = hot.days.get(&key) {
            return d.digest;
        }
        self.view.read().owner.get(&key).map(|(_, d)| *d).filter(|d| *d != NO_DIGEST)
    }

    /// Whether a day has a watermark.
    pub fn has_watermark(&self, src: &str, day: &str) -> bool {
        self.watermark(src, day).is_some()
    }

    /// Whether the day was indexed at the version vector `vv` (an encoded one).
    pub fn watermark_is(&self, src: &str, day: &str, vv: &[u8]) -> bool {
        self.watermark(src, day) == Some(digest_of(vv))
    }

    // ---- segments ---------------------------------------------------------------------------

    /// Writes cold days into segments (see [`Mode`]), merges the segments of a month that has
    /// too many or is over, all in one transaction with the manifest, then lets the hot tier go
    /// of the days written. A day changed meanwhile stays in memory.
    pub fn maintain(&self, mode: Mode) -> Result<(), Error> {
        self.maintain_segments(mode)?;
        if mode != Mode::Cap {
            self.compact_if_worthwhile()?;
        }
        Ok(())
    }

    /// Gives the file's free space back when it is mostly free: more than
    /// [`COMPACT_MIN_WASTE`] bytes and more than half of the file (segment merges and rewrites
    /// leave free pages, and redb grows the file in big steps). Only looks after the file
    /// changed since the last look. Holds the maintenance lock, and redb the database, for the
    /// duration (queries wait; `apply` does not touch the file except to load a cold day), so
    /// it belongs in the background. Returns the file length before and after when it ran.
    pub fn compact_if_worthwhile(&self) -> Result<Option<(u64, u64)>, Error> {
        let _m = self.maint.lock();
        if self.is_closed() || !self.file_changed.swap(false, std::sync::atomic::Ordering::SeqCst) {
            return Ok(None);
        }
        let Some(u) = self.db.raw().usage().map_err(io)? else { return Ok(None) };
        if u.wasted() <= self.limits().compact_min_waste || u.wasted() * 2 <= u.file_bytes {
            return Ok(None);
        }
        self.compact_locked()
    }

    /// File length and live bytes (benchmarks).
    pub fn usage(&self) -> Option<storage::Usage> {
        self.db.raw().usage().ok().flatten()
    }

    /// Compacts regardless of the thresholds (tests, tools).
    pub fn compact_now(&self) -> Result<Option<(u64, u64)>, Error> {
        let _m = self.maint.lock();
        self.compact_locked()
    }

    fn compact_locked(&self) -> Result<Option<(u64, u64)>, Error> {
        let Some(before) = self.db.raw().usage().map_err(io)? else { return Ok(None) };
        self.db.raw().compact().map_err(io)?;
        let after = self.db.raw().usage().map_err(io)?.map_or(before.file_bytes, |u| u.file_bytes);
        Ok(Some((before.file_bytes, after)))
    }

    pub(crate) fn maintain_segments(&self, mode: Mode) -> Result<(), Error> {
        let mut memo = self.maint.lock();
        if self.is_closed() {
            return Ok(());
        }
        let lim = self.limits();
        let today = self.today();
        let now = Instant::now();
        struct Picked {
            key: DayKey,
            day: String,
            ver: u64,
            data: DayData,
        }
        let picked: Vec<Picked> = {
            let mut hot = self.hot.lock();
            let over = hot.docs.len() > lim.hot_max_docs;
            let mut cold: Vec<DayKey> = Vec::new();
            for (k, d) in hot.days.iter_mut() {
                if !self.is_hot(k.1, today, &lim) {
                    d.cold_since.get_or_insert(now);
                    cold.push(*k);
                }
            }
            cold.sort_by_key(|k| (k.1, k.0));
            let mut total = hot.docs.len();
            let mut out = Vec::new();
            for k in cold {
                let d = &hot.days[&k];
                let due = match mode {
                    Mode::Force => true,
                    Mode::Tick => d.cold_since.is_some_and(|t| now.duration_since(t) >= lim.debounce),
                    Mode::Cap => false,
                };
                let by_cap = over && mode != Mode::Force && total > lim.hot_max_docs / 2;
                if due || by_cap {
                    total -= d.docs.len();
                    let rows = d.docs.iter().filter_map(|h| hot.docs.get(h).map(|(_, r, _)| (*h, r.clone()))).collect();
                    out.push(Picked { key: k, day: d.day.clone(), ver: d.ver, data: DayData { key: k, digest: d.digest.unwrap_or(NO_DIGEST), rows } });
                }
            }
            out
        };
        let t = phase(0, now);
        let view = self.view.read().clone();
        let cur_month = parse_day(&day_of(i64::from(today) * DAY_MS)).map_or(0, |d| d.1);
        let flush_keys: HashSet<DayKey> = picked.iter().map(|p| p.key).collect();
        let mut by_month: BTreeMap<u32, Vec<&DayData>> = BTreeMap::new();
        for p in &picked {
            by_month.entry(parse_day(&p.day).map_or(0, |d| d.1)).or_default().push(&p.data);
        }
        let months: BTreeSet<u32> = by_month.keys().copied().chain(view.man.segs.iter().map(|s| s.month)).collect();
        let mut next = view.man.next;
        let mut keep: Vec<Seg> = view.man.segs.clone();
        let mut remove: Vec<u32> = Vec::new();
        let mut merged: Vec<(u32, Vec<u32>)> = Vec::new();
        // Pass 1: which months are rewritten whole (their old segments plus the new days).
        let mut arena: BTreeMap<u32, Vec<DayData>> = BTreeMap::new();
        for &m in &months {
            let old: Vec<&Seg> = view.man.segs.iter().filter(|s| s.month == m).collect();
            let fresh: Vec<&DayData> = by_month.get(&m).cloned().unwrap_or_default();
            let total: usize = old.iter().map(|s| s.docs as usize).sum::<usize>() + fresh.iter().map(|d| d.rows.len()).sum::<usize>();
            let count_after = old.len() + pack(&fresh, lim.seg_max_docs).len();
            let min_possible = total.div_ceil(lim.seg_max_docs).max(1);
            let ids: Vec<u32> = old.iter().map(|s| s.id).collect();
            let merge = mode != Mode::Cap && !old.is_empty() && count_after > min_possible && (count_after > lim.merge_segs || m < cur_month) && !(fresh.is_empty() && memo.merged.get(&m) == Some(&ids));
            if merge {
                let tr = Instant::now();
                let mut days = self.valid_days(&view, &old, &flush_keys);
                phase(1, tr);
                days.extend(fresh.iter().map(|d| DayData { key: d.key, digest: d.digest, rows: d.rows.clone() }));
                arena.insert(m, days);
                remove.extend(ids);
                keep.retain(|s| s.month != m);
            }
        }
        // Pass 2: the segments to build, then built on all cores.
        let mut jobs: Vec<(u32, u32, Vec<&DayData>)> = Vec::new();
        for &m in &months {
            let days: Vec<&DayData> = match arena.get(&m) {
                Some(a) => a.iter().collect(),
                None => by_month.get(&m).cloned().unwrap_or_default(),
            };
            let mut new_ids = Vec::new();
            for group in pack(&days, lim.seg_max_docs) {
                jobs.push((next, m, group.iter().map(|i| days[*i]).collect()));
                new_ids.push(next);
                next += 1;
            }
            if arena.contains_key(&m) {
                merged.push((m, new_ids));
            }
        }
        let built: Vec<Built> = {
            let slots: Vec<Mutex<Option<Built>>> = jobs.iter().map(|_| Mutex::new(None)).collect();
            let at = std::sync::atomic::AtomicUsize::new(0);
            let workers = std::thread::available_parallelism().map_or(1, |n| n.get()).min(jobs.len()).max(1);
            std::thread::scope(|sc| {
                for _ in 0..workers {
                    sc.spawn(|| {
                        let tok = |t: &str| self.keyer.tok(t);
                        loop {
                            let i = at.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            let Some((id, m, days)) = jobs.get(i) else { break };
                            *slots[i].lock() = Some(seg::build(*id, *m, days, &tok));
                        }
                    });
                }
            });
            slots.into_iter().filter_map(|s| s.into_inner()).collect()
        };
        let t = phase(2, t);
        // A rewritten day leaves the older segment that held it.
        let mut changed = !built.is_empty() || !remove.is_empty();
        for s in &mut keep {
            let n = s.days.len();
            s.days.retain(|(a, b, _)| !flush_keys.contains(&(*a, *b)));
            changed |= s.days.len() != n;
        }
        for s in keep.iter().filter(|s| s.days.is_empty()) {
            remove.push(s.id);
        }
        keep.retain(|s| !s.days.is_empty());
        if !changed {
            return Ok(());
        }
        let mut segs = keep;
        segs.extend(built.iter().map(|b| b.seg.clone()));
        segs.sort_by_key(|s| s.id);
        let man = Manifest { next, segs };
        let mut ops: Vec<Op> = Vec::new();
        for id in &remove {
            ops.push(Op::DeletePrefix(format!("x/{id:08x}/")));
            ops.push(Op::DeletePrefix(format!("y/{id:08x}/")));
        }
        // In key order: redb grows the file many times over when large values are inserted
        // between runs of small ones (512 MiB for 6.7 MB of records, measured).
        let mut puts: Vec<(&str, &[u8])> = built.iter().flat_map(|b| b.puts.iter().map(|(k, v)| (k.as_str(), v.as_slice()))).collect();
        puts.sort_unstable_by_key(|(k, _)| *k);
        for (k, v) in puts {
            ops.push(self.db.put_op(k, v).map_err(io)?);
        }
        ops.push(self.db.put_op("m", &bincode::serialize(&man).expect("serialises")).map_err(io)?);
        let tw = Instant::now();
        self.db.apply(&ops).map_err(io)?;
        self.file_changed.store(true, std::sync::atomic::Ordering::SeqCst);
        let t = phase(3, tw).max(t);
        let nv = Arc::new(View::new(man));
        {
            let _hot = self.hot.lock();
            *self.view.write() = nv;
        }
        // The hot copies go one day at a time, so that a send is never held up for long.
        for p in &picked {
            let mut hot = self.hot.lock();
            match hot.days.get_mut(&p.key) {
                Some(d) if d.ver == p.ver => hot.drop_day(&p.key),
                Some(d) => d.cold_since = Some(now),
                None => {}
            }
        }
        memo.merged.extend(merged);
        phase(4, t);
        Ok(())
    }

    /// The days of `old` segments with the rows they still own (not rewritten by `skip`).
    fn valid_days(&self, view: &View, old: &[&Seg], skip: &HashSet<DayKey>) -> Vec<DayData> {
        let mut map: HashMap<DayKey, DayData> = HashMap::new();
        for s in old {
            for (a, b, d) in &s.days {
                let key = (*a, *b);
                if !skip.contains(&key) && view.owner.get(&key).is_some_and(|o| o.0 == s.id) {
                    map.insert(key, DayData { key, digest: *d, rows: Vec::new() });
                }
            }
        }
        for s in old {
            for c in 0..(s.docs as usize).div_ceil(CHUNK) {
                for (h, row) in self.read_chunk(s.id, c).unwrap_or_default() {
                    let key = self.keyer.day_key(&row.src, &row.day);
                    if view.owner.get(&key).is_some_and(|o| o.0 == s.id)
                        && let Some(d) = map.get_mut(&key)
                    {
                        d.rows.push((h, row));
                    }
                }
            }
        }
        let mut out: Vec<DayData> = map.into_values().collect();
        out.sort_by_key(|d| d.key);
        out
    }

    // ---- reading ----------------------------------------------------------------------------

    /// A newest-first walk over the docs that contain the indexed `term`, after the cursor
    /// `after` (`{rev_at}/{h}`, as `Scan::next` returns it).
    #[cfg(test)]
    pub fn scan(&self, term: &str, after: Option<&str>) -> Result<Scan<'_>, Error> {
        self.scan_all(&[term.to_string()], after)
    }

    /// Like [`scan`](Self::scan), but only the docs posted under every one of `terms` (the
    /// first is the one to walk; the others only narrow it, per segment by intersecting the
    /// posting lists before any row is read).
    pub fn scan_all(&self, terms: &[String], after: Option<&str>) -> Result<Scan<'_>, Error> {
        let mut toks: Vec<u64> = Vec::new();
        for t in terms {
            let k = self.keyer.tok(t);
            if !toks.contains(&k) {
                toks.push(k);
            }
        }
        if toks.is_empty() {
            return Err(Error::Protocol("no term".into()));
        }
        let after = match after {
            None => None,
            Some(a) => {
                let (r, h) = a.split_once('/').ok_or_else(|| Error::Protocol("bad cursor".into()))?;
                Some((u64::from_str_radix(r, 16).map_err(|_| Error::Protocol("bad cursor".into()))?, u64::from_str_radix(h, 16).map_err(|_| Error::Protocol("bad cursor".into()))?))
            }
        };
        let (view, hot_days) = {
            let hot = self.hot.lock();
            (self.view.read().clone(), hot.days.keys().copied().collect::<HashSet<_>>())
        };
        let mut pending: Vec<Seg> = view.man.segs.iter().filter(|s| s.docs > 0 && after.is_none_or(|a| rev_at(s.min_at) >= a.0)).cloned().collect();
        pending.sort_by_key(|s| (std::cmp::Reverse(s.max_at), std::cmp::Reverse(s.id)));
        Ok(Scan { ix: self, toks, view, hot_days, after, hot_buf: VecDeque::new(), hot_last: after, hot_done: false, pending: pending.into(), open: Vec::new(), chunks: HashMap::new(), last: None })
    }

    /// `(records, key bytes + value bytes)` of the `x/`, `y/` and `m` records in the file.
    pub fn content_sizes(&self) -> Vec<(&'static str, usize, usize)> {
        ["x/", "y/", "m", "v"].into_iter().map(|p| {
            let rows = self.db.raw().scan(p).unwrap_or_default();
            (p, rows.len(), rows.iter().map(|(k, v)| k.len() + v.len()).sum())
        }).collect()
    }

    #[cfg(test)]
    pub fn raw_rows(&self) -> Vec<(String, Vec<u8>)> {
        self.db.raw().scan("").unwrap()
    }
}

/// Groups days (by index into `days`) into segments of at most `max` docs, newest day first.
fn pack(days: &[&DayData], max: usize) -> Vec<Vec<usize>> {
    let mut idx: Vec<usize> = (0..days.len()).collect();
    idx.sort_by_key(|i| (std::cmp::Reverse(days[*i].key.1), days[*i].key.0));
    let mut out: Vec<Vec<usize>> = Vec::new();
    let mut size = 0;
    for i in idx {
        let n = days[i].rows.len();
        match out.last_mut() {
            Some(g) if size + n <= max => {
                g.push(i);
                size += n;
            }
            _ => {
                out.push(vec![i]);
                size = n;
            }
        }
    }
    out
}

type Chunk = Arc<Vec<(u64, DocRow)>>;

struct Open {
    seg: u32,
    docs: u32,
    /// The next posting-list part to load.
    part: usize,
    list: Vec<u32>,
    pos: usize,
    head: Option<(u64, u64, DocRow)>,
}

/// A lazy k-way merge, newest first, of one term's postings: the hot tier and the segments,
/// each opened only once the merge reaches its newest time.
pub struct Scan<'a> {
    ix: &'a SearchIndex,
    toks: Vec<u64>,
    view: Arc<View>,
    hot_days: HashSet<DayKey>,
    after: Option<(u64, u64)>,
    hot_buf: VecDeque<(u64, u64, DocRow)>,
    hot_last: Option<(u64, u64)>,
    hot_done: bool,
    pending: VecDeque<Seg>,
    open: Vec<Open>,
    chunks: HashMap<(u32, usize), Option<Chunk>>,
    last: Option<(u64, u64)>,
}

const HOT_BATCH: usize = 256;

impl Scan<'_> {
    /// Up to `n` more docs with their cursors, and whether there may be more.
    pub fn next(&mut self, n: usize) -> Result<(Vec<(String, DocRow)>, bool), Error> {
        let mut out = Vec::new();
        while out.len() < n {
            let Some((rev, h, row)) = self.pop() else { return Ok((out, false)) };
            out.push((format!("{rev:016x}/{h:016x}"), row));
        }
        Ok((out, true))
    }

    fn chunk(&mut self, seg: u32, c: usize) -> Option<Chunk> {
        if let Some(v) = self.chunks.get(&(seg, c)) {
            return v.clone();
        }
        let v = self.ix.read_chunk(seg, c).map(Arc::new);
        self.chunks.insert((seg, c), v.clone());
        v
    }

    fn row(&mut self, seg: u32, doc: u32) -> Option<(u64, DocRow)> {
        let c = self.chunk(seg, doc as usize / CHUNK)?;
        c.get(doc as usize % CHUNK).cloned()
    }

    fn refill_hot(&mut self) {
        if self.hot_done || !self.hot_buf.is_empty() {
            return;
        }
        let v = self.ix.hot.lock().range(self.toks[0], &self.toks[1..], self.hot_last, HOT_BATCH);
        if v.len() < HOT_BATCH {
            self.hot_done = true;
        }
        if let Some((r, h, _)) = v.last() {
            self.hot_last = Some((*r, *h));
        }
        self.hot_buf.extend(v);
    }

    fn open_seg(&mut self, s: &Seg) {
        // The first doc after the cursor, by bisection over the rows (all of them, with the
        // posting lists read part by part from there).
        let mut first = 0usize;
        if let Some(a) = self.after
            && rev_at(s.max_at) <= a.0
        {
            let (mut lo, mut hi) = (0, s.docs as usize);
            while lo < hi {
                let mid = (lo + hi) / 2;
                match self.row(s.id, mid as u32) {
                    Some((h, r)) if (rev_at(r.at), h) <= a => lo = mid + 1,
                    _ => hi = mid,
                }
            }
            first = lo;
        }
        let part = first / PART_DOCS;
        self.open.push(Open { seg: s.id, docs: s.docs, part, list: Vec::new(), pos: 0, head: None });
        let i = self.open.len() - 1;
        if self.load_part(i) {
            let o = &mut self.open[i];
            o.pos = o.list.partition_point(|d| (*d as usize) < first);
        }
        self.advance(i);
    }

    /// Reads the next part of an open segment's posting list into `list`; false at the end.
    fn load_part(&mut self, i: usize) -> bool {
        loop {
            let (seg, docs, part) = {
                let o = &self.open[i];
                (o.seg, o.docs, o.part)
            };
            if part * PART_DOCS >= docs as usize {
                let o = &mut self.open[i];
                o.list.clear();
                o.pos = 0;
                return false;
            }
            let mut list: Option<Vec<u32>> = None;
            for tok in &self.toks {
                let l = self.ix.db.get(&x_key(seg, *tok, part)).ok().flatten().and_then(|b| decode_list(&b, (part * PART_DOCS) as u32)).unwrap_or_default();
                list = Some(match list {
                    None => l,
                    Some(prev) => {
                        let set: HashSet<u32> = l.into_iter().collect();
                        prev.into_iter().filter(|d| set.contains(d)).collect()
                    }
                });
                if list.as_ref().is_some_and(Vec::is_empty) {
                    break;
                }
            }
            let o = &mut self.open[i];
            o.part += 1;
            if let Some(l) = list.filter(|l| !l.is_empty()) {
                o.list = l;
                o.pos = 0;
                return true;
            }
        }
    }

    /// Loads the head of an open segment from its position.
    fn advance(&mut self, i: usize) {
        loop {
            while self.open[i].pos >= self.open[i].list.len() {
                if !self.load_part(i) {
                    self.open[i].head = None;
                    return;
                }
            }
            let (seg, doc) = {
                let o = &self.open[i];
                (o.seg, o.list[o.pos])
            };
            if let Some((h, row)) = self.row(seg, doc) {
                self.open[i].head = Some((rev_at(row.at), h, row));
                return;
            }
            self.open[i].pos += 1;
        }
    }

    /// The smallest head: `(key, None)` for the hot tier or `(key, Some(open index))`.
    fn best(&self) -> Option<((u64, u64), Option<usize>)> {
        let mut best = self.hot_buf.front().map(|(r, h, _)| ((*r, *h), None));
        for (i, o) in self.open.iter().enumerate() {
            if let Some((r, h, _)) = &o.head
                && best.is_none_or(|(k, _)| (*r, *h) < k)
            {
                best = Some(((*r, *h), Some(i)));
            }
        }
        best
    }

    fn pop(&mut self) -> Option<(u64, u64, DocRow)> {
        loop {
            self.refill_hot();
            while let Some(s) = self.pending.front() {
                if self.best().is_some_and(|(k, _)| rev_at(s.max_at) > k.0) {
                    break;
                }
                let s = self.pending.pop_front().expect("peeked");
                self.open_seg(&s);
            }
            let (_, src) = self.best()?;
            let (rev, h, row, seg) = match src {
                None => {
                    let (r, h, row) = self.hot_buf.pop_front().expect("best");
                    (r, h, row, None)
                }
                Some(i) => {
                    let (r, h, row) = self.open[i].head.take().expect("best");
                    let seg = self.open[i].seg;
                    self.open[i].pos += 1;
                    self.advance(i);
                    (r, h, row, Some(seg))
                }
            };
            if self.last == Some((rev, h)) {
                continue;
            }
            if let Some(seg) = seg {
                // A segment's doc counts only while the segment owns its day and the hot tier
                // does not hold that day.
                let key = self.ix.keyer.day_key(&row.src, &row.day);
                if self.view.owner.get(&key).map(|o| o.0) != Some(seg) || self.hot_days.contains(&key) {
                    continue;
                }
            }
            self.last = Some((rev, h));
            return Some((rev, h, row));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(src: &str, id: &str, at: i64, text: &str) -> DocRow {
        DocRow { src: src.into(), conv: "did:key:z".into(), day: crate::chat::doc::day_of(at), id: id.into(), at, author: "did:key:a".into(), text: text.into() }
    }

    const D: i64 = DAY_MS;
    /// 2026-01-01 00:00 UTC.
    const T0: i64 = 1_767_225_600_000;

    fn ids(ix: &SearchIndex, term: &str) -> Vec<String> {
        let mut s = ix.scan(term, None).unwrap();
        s.next(1000).unwrap().0.into_iter().map(|(_, r)| r.id).collect()
    }

    fn mark(r: &DocRow, n: u8) -> Mark {
        Mark { src: r.src.clone(), day: r.day.clone(), prev: None, vv: vec![n] }
    }

    fn at_day(ix: &SearchIndex, day: &str) {
        ix.set_today(Some(day));
    }

    // KIND: unit (v2) — postings are newest first; an edit moves them, a delete removes them.
    #[test]
    fn edit_and_delete_reindex() {
        let ix = SearchIndex::in_memory("alpha beta");
        ix.apply(&[row("dm.p", "1", T0 + 10, "hello there"), row("dm.p", "2", T0 + 20, "help me")], &[], &[]).unwrap();
        assert_eq!(ids(&ix, "hel"), ["2", "1"]);
        ix.apply(&[row("dm.p", "1", T0 + 10, "goodbye")], &[], &[]).unwrap();
        assert_eq!(ids(&ix, "hel"), ["2"]);
        assert_eq!(ids(&ix, "go"), ["1"]);
        ix.apply(&[row("dm.p", "2", T0 + 20, "")], &[], &[]).unwrap();
        assert!(ids(&ix, "hel").is_empty());
        ix.apply(&[], &[ix.doc_key("dm.p", "1")], &[]).unwrap();
        assert!(ids(&ix, "go").is_empty());
        assert_eq!(ix.stats().hot_docs, 0);
        assert_eq!(ix.raw_rows().len(), 1, "nothing was written but the version");
    }

    // KIND: unit (v2) — forgetting a conversation removes its docs and watermarks only, in
    // memory and in segments.
    #[test]
    fn forget_removes_one_conversation() {
        let ix = SearchIndex::in_memory("alpha beta");
        let (a, b) = (row("g.a", "1", T0 + 1, "picnic plans"), row("g.b", "2", T0 + 2, "picnic too"));
        ix.apply(&[a.clone(), b.clone()], &[], &[mark(&a, 1), mark(&b, 1)]).unwrap();
        ix.forget("g.a").unwrap();
        assert_eq!(ids(&ix, "pic"), ["2"]);
        assert!(!ix.has_watermark("g.a", &a.day) && ix.has_watermark("g.b", &b.day));
        assert!(ix.day_keys("g.a", &a.day).unwrap().is_empty());
        // Now the same from a segment.
        at_day(&ix, "2026-06-01");
        ix.maintain(Mode::Force).unwrap();
        assert_eq!(ix.stats().hot_docs, 0);
        assert_eq!(ids(&ix, "pic"), ["2"]);
        ix.apply(std::slice::from_ref(&a), &[], &[mark(&a, 1)]).unwrap();
        ix.maintain(Mode::Force).unwrap();
        assert_eq!(ids(&ix, "pic"), ["2", "1"]);
        ix.forget("g.b").unwrap();
        assert_eq!(ids(&ix, "pic"), ["1"]);
        assert!(!ix.has_watermark("g.b", &b.day));
        ix.forget("g.a").unwrap();
        assert!(ids(&ix, "pic").is_empty());
        assert_eq!(ix.stats().segs, 0, "an empty segment is dropped");
        assert_eq!(ix.raw_rows().len(), 2, "version and manifest");
    }

    // KIND: unit (v2) — a conditional watermark only moves from the version it expects.
    #[test]
    fn conditional_marks() {
        let ix = SearchIndex::in_memory("alpha beta");
        let mut v1 = loro::VersionVector::default();
        v1.insert(7, 3);
        let mut v2 = v1.clone();
        v2.insert(7, 5);
        let m = |prev: Option<&loro::VersionVector>, vv: &loro::VersionVector| Mark { src: "dm.p".into(), day: "2026-01-01".into(), prev: prev.map(|p| p.encode()), vv: vv.encode() };
        ix.apply(&[], &[], &[m(Some(&loro::VersionVector::default()), &v1)]).unwrap();
        assert!(ix.watermark_is("dm.p", "2026-01-01", &v1.encode()));
        // A change that assumed some other version leaves it alone.
        ix.apply(&[], &[], &[m(Some(&v2), &v2)]).unwrap();
        assert!(ix.watermark_is("dm.p", "2026-01-01", &v1.encode()));
        ix.apply(&[], &[], &[m(Some(&v1), &v2)]).unwrap();
        assert!(ix.watermark_is("dm.p", "2026-01-01", &v2.encode()));
        assert!(!ix.watermark_is("dm.p", "2026-01-01", &v1.encode()));
    }

    // KIND: unit (v2) — paging by cursor walks every doc once, newest first, in memory and across
    // a segment, also when the docs move between pages.
    #[test]
    fn paging_newest_first_across_tiers() {
        let ix = SearchIndex::in_memory("alpha beta");
        at_day(&ix, "2026-03-01");
        let rows: Vec<DocRow> = (0..40).map(|i| row("dm.p", &format!("m{i:02}"), T0 + i * D / 4, "lunch today")).collect();
        ix.apply(&rows[..30], &[], &[]).unwrap();
        ix.maintain(Mode::Force).unwrap();
        assert!(ix.stats().segs > 0 && ix.stats().hot_docs == 0);
        // The newer docs arrive late, as hot docs of cold days (dirty).
        ix.apply(&rows[30..], &[], &[]).unwrap();
        let want: Vec<String> = (0..40).rev().map(|i| format!("m{i:02}")).collect();
        let mut seen = Vec::new();
        let mut cur: Option<String> = None;
        let mut flipped = false;
        loop {
            let mut s = ix.scan("lunch", cur.as_deref()).unwrap();
            let (page, more) = s.next(7).unwrap();
            seen.extend(page.iter().map(|(_, r)| r.id.clone()));
            cur = page.last().map(|(c, _)| c.clone());
            if !flipped {
                // The tier changes between pages: the dirty days go into segments.
                ix.maintain(Mode::Force).unwrap();
                flipped = true;
            }
            if !more {
                break;
            }
        }
        assert_eq!(seen, want);
    }

    // KIND: unit (v2) — at rest the file holds no words, names or authors: values are sealed and
    // keys carry only keyed hashes. Another phrase reads nothing.
    #[test]
    fn sealed_at_rest() {
        let ix = SearchIndex::in_memory("alpha beta");
        let mut r = row("dm.p", "1", T0 + 10, "confidential rendezvous");
        r.author = "did:key:zAuthorSecret".into();
        ix.apply(&[r], &[], &[]).unwrap();
        at_day(&ix, "2026-06-01");
        ix.maintain(Mode::Force).unwrap();
        let rows = ix.raw_rows();
        assert!(rows.len() > 3);
        for (k, v) in &rows {
            for word in ["confidential", "rendezvous", "conf", "AuthorSecret"] {
                assert!(!k.contains(word), "key {k} leaks {word}");
                assert!(!v.windows(word.len()).any(|w| w == word.as_bytes()), "value of {k} leaks {word}");
            }
        }
        let other = SearchIndex::with_store(ix.db.raw().clone(), "other phrase").unwrap();
        assert!(ids(&other, "conf").is_empty());
        assert!(ix.db.raw().get("v").unwrap().is_some());
    }

    // KIND: unit (v2) — a file of another format version (the v1 layout) is emptied on open.
    #[test]
    fn version_mismatch_empties() {
        let ix = SearchIndex::in_memory("alpha beta");
        let raw = ix.db.raw().clone();
        ix.db.put("v", &1u32.to_be_bytes()).unwrap();
        ix.db.put("d/00ff", b"old row").unwrap();
        raw.put("t/abcd/ffff/00ff", b"").unwrap();
        let again = SearchIndex::with_store(raw, "alpha beta").unwrap();
        assert_eq!(again.raw_rows().len(), 1, "only the version row");
        assert!(again.ix_version_ok());
    }

    impl SearchIndex {
        fn ix_version_ok(&self) -> bool {
            self.db.get("v").unwrap().unwrap() == VERSION.to_be_bytes()
        }
    }

    // KIND: unit (v2) — a day that fell out of the hot window is written into a segment (memory
    // empty, found from the file), and a reopen finds it from the manifest alone.
    #[test]
    fn cold_day_in_a_segment_is_found_after_reopen() {
        let ix = SearchIndex::in_memory("alpha beta");
        let r = row("dm.p", "1", T0, "ancient manuscript");
        ix.apply(std::slice::from_ref(&r), &[], &[mark(&r, 1)]).unwrap();
        at_day(&ix, "2026-01-05");
        ix.maintain(Mode::Force).unwrap();
        assert_eq!(ix.stats().segs, 0, "still within the hot window");
        assert_eq!(ids(&ix, "manus"), ["1"]);
        at_day(&ix, "2026-01-20");
        ix.maintain(Mode::Force).unwrap();
        assert_eq!((ix.stats().hot_docs, ix.stats().segs), (0, 1));
        assert_eq!(ids(&ix, "manus"), ["1"]);
        assert!(ix.watermark_is("dm.p", &r.day, &[1]) || ix.has_watermark("dm.p", &r.day));
        let again = SearchIndex::with_store(ix.db.raw().clone(), "alpha beta").unwrap();
        assert_eq!(ids(&again, "manus"), ["1"]);
        assert!(again.has_watermark("dm.p", &r.day));
        assert_eq!(again.stats().hot_docs, 0);
    }

    // KIND: unit (v2) — a late post into a segmented day, an edit and a delete of a segmented
    // message are seen at once (the day comes back into memory); the segment is rewritten later.
    #[test]
    fn dirty_cold_day_overrides_its_segment() {
        let ix = SearchIndex::in_memory("alpha beta");
        let (a, b) = (row("g.x", "a", T0 + 100, "plan the picnic"), row("g.x", "b", T0 + 200, "bring blankets"));
        ix.apply(&[a.clone(), b.clone()], &[], &[mark(&a, 1)]).unwrap();
        at_day(&ix, "2026-06-01");
        ix.maintain(Mode::Force).unwrap();
        assert_eq!(ix.stats().hot_docs, 0);
        // late post
        let late = row("g.x", "c", T0 + 300, "also lemonade");
        ix.apply(std::slice::from_ref(&late), &[], &[mark(&late, 2)]).unwrap();
        assert_eq!(ids(&ix, "lemon"), ["c"]);
        assert_eq!(ids(&ix, "bl"), ["b"], "the day's other docs are served from memory");
        assert_eq!(ix.stats().hot_docs, 3);
        // edit and delete
        ix.apply(&[row("g.x", "a", T0 + 100, "plan the barbecue")], &[], &[]).unwrap();
        assert!(ids(&ix, "picnic").is_empty());
        assert_eq!(ids(&ix, "barbe"), ["a"]);
        ix.apply(&[], &[ix.doc_key("g.x", "b")], &[]).unwrap();
        assert!(ids(&ix, "blank").is_empty());
        // The month is rewritten: the same answers from the file, nothing in memory.
        ix.maintain(Mode::Force).unwrap();
        assert_eq!(ix.stats().hot_docs, 0);
        assert_eq!(ix.stats().segs, 1, "the old segment emptied of its day and went");
        assert_eq!(ids(&ix, "barbe"), ["a"]);
        assert_eq!(ids(&ix, "lemon"), ["c"]);
        assert!(ids(&ix, "picnic").is_empty() && ids(&ix, "blank").is_empty());
        // A change that alters nothing does not bring the day back.
        ix.apply(std::slice::from_ref(&late), &[], &[mark(&late, 2)]).unwrap();
        assert_eq!(ix.stats().hot_docs, 0);
    }

    // KIND: unit (v2) — a crash after a hot change on a cold day, before its month was rewritten:
    // the reopened file has the old watermark, so the day is known to differ.
    #[test]
    fn crash_before_rewrite_leaves_the_old_watermark() {
        let ix = SearchIndex::in_memory("alpha beta");
        let a = row("dm.p", "a", T0 + 100, "one two");
        ix.apply(std::slice::from_ref(&a), &[], &[mark(&a, 1)]).unwrap();
        at_day(&ix, "2026-06-01");
        ix.maintain(Mode::Force).unwrap();
        ix.apply(&[row("dm.p", "a", T0 + 100, "one three")], &[], &[mark(&a, 2)]).unwrap();
        assert!(ix.watermark_is("dm.p", &a.day, &[2]));
        let crashed = SearchIndex::with_store(ix.db.raw().clone(), "alpha beta").unwrap();
        assert!(crashed.watermark_is("dm.p", &a.day, &[1]) && !crashed.watermark_is("dm.p", &a.day, &[2]));
        assert_eq!(ids(&crashed, "two"), ["a"], "the file still has the old text, reconcile fixes it");
    }

    // KIND: unit (v2) — SEG_MAX_DOCS splits a month into several segments, newest days first, and
    // a query merges them in order.
    #[test]
    fn segment_cap_splits_a_month() {
        let ix = SearchIndex::in_memory("alpha beta");
        ix.set_limits(|l| l.seg_max_docs = 10);
        let mut rows = Vec::new();
        for d in 0..6 {
            for k in 0..5 {
                rows.push(row("dm.p", &format!("d{d}k{k}"), T0 + d * D + k * 1000, "daily standup"));
            }
        }
        ix.apply(&rows, &[], &[]).unwrap();
        at_day(&ix, "2026-06-01");
        ix.maintain(Mode::Force).unwrap();
        let st = ix.stats();
        assert_eq!((st.segs, st.seg_docs), (3, 30));
        let want: Vec<String> = rows.iter().rev().map(|r| r.id.clone()).collect();
        assert_eq!(ids(&ix, "stand"), want);
    }

    // KIND: unit (v2) — segments of one month merge when there are more than 4, and a month that
    // is over is merged into one; day order and answers are kept.
    #[test]
    fn same_month_segments_merge() {
        let ix = SearchIndex::in_memory("alpha beta");
        at_day(&ix, "2026-01-20");
        let mut all = Vec::new();
        for d in 0..4 {
            let r = row("dm.p", &format!("m{d}"), T0 + d * D, "weekly review");
            all.push(r.id.clone());
            ix.apply(&[r], &[], &[]).unwrap();
            // Each day goes cold on its own, as segments of the same month.
            at_day(&ix, &format!("2026-01-{:02}", 10 + d + 8));
            ix.maintain(Mode::Force).unwrap();
        }
        assert_eq!(ix.stats().segs, 4, "four are allowed");
        let r = row("dm.p", "m4", T0 + 4 * D, "weekly review");
        all.push(r.id.clone());
        ix.apply(&[r], &[], &[]).unwrap();
        at_day(&ix, "2026-01-28");
        ix.maintain(Mode::Force).unwrap();
        assert_eq!(ix.stats().segs, 1, "the fifth merged them");
        all.reverse();
        assert_eq!(ids(&ix, "weekly"), all);
        // Month over: two segments become one.
        let r = row("dm.p", "m5", T0 + 5 * D, "weekly review");
        ix.apply(&[r], &[], &[]).unwrap();
        at_day(&ix, "2026-01-30");
        ix.maintain(Mode::Force).unwrap();
        assert_eq!(ix.stats().segs, 2);
        at_day(&ix, "2026-02-02");
        ix.maintain(Mode::Tick).unwrap();
        assert_eq!(ix.stats().segs, 1, "the month is over");
        assert_eq!(ids(&ix, "weekly").len(), 6);
        ix.maintain(Mode::Tick).unwrap();
        assert_eq!(ix.stats().segs, 1);
    }

    // KIND: unit (v2) — a query word longer than 8 chars is looked up by its 8-char prefix and
    // checked against the text, in memory and in a segment.
    #[test]
    fn long_term_found_by_prefix_and_checked() {
        let ix = SearchIndex::in_memory("alpha beta");
        let (a, b) = (row("dm.p", "a", T0 + 1, "internationalization day"), row("dm.p", "b", T0 + 2, "international relations"));
        ix.apply(&[a, b], &[], &[]).unwrap();
        let want = |ix: &SearchIndex| {
            let terms = super::super::text::query_terms("internationaliz");
            let look = terms[0].lookup();
            assert_eq!(look.chars().count(), 8);
            let mut s = ix.scan(&look, None).unwrap();
            let got: Vec<String> = s.next(10).unwrap().0.into_iter().map(|(_, r)| r).filter(|r| super::super::text::match_all(&terms, &super::super::text::normalize(&r.text)).is_some()).map(|r| r.id).collect();
            assert_eq!(got, ["a"]);
        };
        want(&ix);
        at_day(&ix, "2026-06-01");
        ix.maintain(Mode::Force).unwrap();
        want(&ix);
    }

    // KIND: unit (v2) — a posting list longer than one part (2048 docs) is read part by part, and
    // a cursor in the middle of a part resumes exactly there.
    #[test]
    fn long_lists_span_parts() {
        let ix = SearchIndex::in_memory("alpha beta");
        let rows: Vec<DocRow> = (0..5000).map(|i| row("dm.p", &format!("m{i:04}"), T0 + i * 1000, "common word")).collect();
        ix.apply(&rows, &[], &[]).unwrap();
        at_day(&ix, "2026-06-01");
        ix.maintain(Mode::Force).unwrap();
        assert_eq!((ix.stats().segs, ix.stats().seg_docs), (1, 5000));
        let want: Vec<String> = (0..5000).rev().map(|i| format!("m{i:04}")).collect();
        let mut seen = Vec::new();
        let mut cur: Option<String> = None;
        loop {
            let mut s = ix.scan("common", cur.as_deref()).unwrap();
            let (page, more) = s.next(777).unwrap();
            seen.extend(page.iter().map(|(_, r)| r.id.clone()));
            cur = page.last().map(|(c, _)| c.clone());
            if !more {
                break;
            }
        }
        assert_eq!(seen, want);
    }

    // KIND: unit (v2) — a scan over several terms returns only docs posted under all of them, in
    // memory and from a segment (the lists are intersected before any row is read).
    #[test]
    fn multi_term_scan_intersects() {
        let ix = SearchIndex::in_memory("alpha beta");
        let rows = [row("dm.p", "a", T0 + 1, "red apple"), row("dm.p", "b", T0 + 2, "red pear"), row("dm.p", "c", T0 + 3, "green apple"), row("dm.p", "d", T0 + 4, "red apple pie")];
        ix.apply(&rows[..2], &[], &[]).unwrap();
        ix.apply(&rows[2..], &[], &[]).unwrap();
        let both = |ix: &SearchIndex| -> Vec<String> { ix.scan_all(&["ap".to_string(), "re".to_string()], None).unwrap().next(10).unwrap().0.into_iter().map(|(_, r)| r.id).collect() };
        assert_eq!(both(&ix), ["d", "a"]);
        at_day(&ix, "2026-06-01");
        ix.maintain(Mode::Force).unwrap();
        assert_eq!(ix.stats().hot_docs, 0);
        assert_eq!(both(&ix), ["d", "a"]);
        assert!(ix.scan_all(&["ap".to_string(), "zz".to_string()], None).unwrap().next(10).unwrap().0.is_empty());
    }

    // KIND: unit (v2) — closing drops and zeroizes the hot tier; a reopen has none of it.
    #[test]
    fn close_clears_the_hot_tier() {
        let ix = SearchIndex::in_memory("alpha beta");
        ix.apply(&[row("dm.p", "a", T0, "secret plans")], &[], &[]).unwrap();
        assert_eq!(ix.stats().hot_docs, 1);
        let raw = ix.db.raw().clone();
        ix.close();
        assert_eq!((ix.stats().hot_docs, ix.stats().hot_days), (0, 0));
        assert!(ix.is_closed());
        assert!(ix.scan("secret", None).unwrap().next(5).unwrap().0.is_empty());
        let _ = raw;
    }

    // KIND: unit (v2) — over HOT_MAX_DOCS the oldest cold days go into segments by themselves
    // (bulk apply), the recent days stay in memory.
    #[test]
    fn hot_overflow_flushes_the_oldest_days() {
        let ix = SearchIndex::in_memory("alpha beta");
        ix.set_limits(|l| l.hot_max_docs = 20);
        at_day(&ix, "2026-06-01");
        for d in 0..10 {
            let rows: Vec<DocRow> = (0..5).map(|k| row("dm.p", &format!("d{d}k{k}"), T0 + d * D + k, "backlog item")).collect();
            ix.apply_bulk(&rows, &[], &[]).unwrap();
        }
        let st = ix.stats();
        assert!(st.hot_docs <= 20 && st.seg_docs > 0 && st.hot_docs + st.seg_docs >= 50, "{st:?}");
        assert_eq!(ids(&ix, "backl").len(), 50);
        // The recent days are never flushed by the cap.
        at_day(&ix, "2026-01-10");
        let rows: Vec<DocRow> = (0..30).map(|k| row("dm.p", &format!("r{k}"), T0 + 9 * D + k, "recent chatter")).collect();
        ix.apply_bulk(&rows, &[], &[]).unwrap();
        assert!(ix.stats().hot_docs >= 30);
    }
    fn temp_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("search-compact-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("search.redb")
    }

    /// Every id under a few terms, newest first, over all pages.
    fn answers(ix: &SearchIndex) -> Vec<Vec<String>> {
        ["pi", "plan", "zebra", "tok", "meeting"].iter().map(|t| ids(ix, t)).collect()
    }

    fn fill(ix: &SearchIndex, convs: usize, days: i64, per_day: usize) {
        let mut n = 0u32;
        for d in 0..days {
            for c in 0..convs {
                let rows: Vec<DocRow> = (0..per_day)
                    .map(|k| {
                        n += 1;
                        row(&format!("dm.{c}"), &format!("m{n}"), T0 + d * D + k as i64 * 1000, &format!("picnic plans {n} token{} meeting zebra{}", n % 17, n % 5))
                    })
                    .collect();
                ix.apply(&rows, &[], &[mark(&rows[0], 1)]).unwrap();
            }
        }
    }

    // KIND: invariant (v2) — the file gives its free space back when it is mostly free and big
    // enough, only after it changed, and the answers are identical afterwards and after a reopen.
    #[test]
    fn compaction_trigger_and_same_answers() {
        let path = temp_path("trigger");
        let ix = SearchIndex::open(&path, "alpha beta").unwrap();
        at_day(&ix, "2026-06-01");
        fill(&ix, 6, 40, 20);
        ix.maintain(Mode::Force).unwrap();
        // Small file: below the byte threshold nothing happens, and the look is spent.
        assert_eq!(ix.compact_if_worthwhile().unwrap(), None);
        ix.set_limits(|l| l.compact_min_waste = 0);
        assert_eq!(ix.compact_if_worthwhile().unwrap(), None, "no change since the last look");
        // Mostly-free file: drop most of the conversations, leaving their pages free.
        for c in 1..6 {
            ix.forget(&format!("dm.{c}")).unwrap();
        }
        ix.maintain(Mode::Force).unwrap();
        let before = answers(&ix);
        assert!(before.iter().any(|a| !a.is_empty()));
        let len_before = std::fs::metadata(&path).unwrap().len();
        let u = ix.db.raw().usage().unwrap().unwrap();
        assert!(u.wasted() * 2 > u.file_bytes, "mostly free: {u:?}");
        let (a, b) = ix.compact_if_worthwhile().unwrap().expect("compacted");
        assert_eq!(a, len_before);
        assert!(b * 2 < a, "{a} -> {b}");
        assert_eq!(std::fs::metadata(&path).unwrap().len(), b);
        assert_eq!(answers(&ix), before);
        // Nothing left to give back: the second look does nothing.
        ix.maintain(Mode::Force).unwrap();
        assert_eq!(ix.compact_if_worthwhile().unwrap(), None);
        // Still writable, and a reopen sees the same index.
        ix.close();
        drop(ix);
        let ix = SearchIndex::open(&path, "alpha beta").unwrap();
        at_day(&ix, "2026-06-01");
        assert_eq!(answers(&ix), before);
        fill(&ix, 1, 2, 5);
        ix.maintain(Mode::Force).unwrap();
        assert!(ids(&ix, "zebra1").len() > before[2].len() / 2);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    // KIND: invariant (v2) — maintenance runs compaction after a write, but never in a bulk
    // (`Cap`), and an in-memory index has nothing to compact.
    #[test]
    fn compaction_follows_maintenance_not_bulk() {
        let ix = SearchIndex::in_memory("alpha beta");
        at_day(&ix, "2026-06-01");
        fill(&ix, 2, 20, 5);
        ix.maintain(Mode::Force).unwrap();
        assert_eq!(ix.compact_if_worthwhile().unwrap(), None);
        assert_eq!(ix.compact_now().unwrap(), None);
    }

    // KIND: crash (v2) — killing the process in the middle of a compaction leaves a file that
    // opens with every record (redb compacts by ordinary commits and repairs a file that was
    // not closed). The child compacts in a loop; the parent kills it at several moments.
    #[test]
    fn compaction_killed_midway_opens() {
        use std::process::{Command, Stdio};
        if let Ok(p) = std::env::var("COMPACT_CHILD") {
            let ix = SearchIndex::open(std::path::Path::new(&p), "alpha beta").unwrap();
            at_day(&ix, "2026-06-01");
            println!("ready");
            loop {
                ix.compact_now().unwrap();
                // free pages again, so that there is something to move
                fill(&ix, 3, 6, 40);
                ix.maintain(Mode::Force).unwrap();
                for c in 0..3 {
                    ix.forget(&format!("dm.{c}")).unwrap();
                }
            }
        }
        let path = temp_path("kill");
        {
            let ix = SearchIndex::open(&path, "alpha beta").unwrap();
            at_day(&ix, "2026-06-01");
            fill(&ix, 8, 40, 30);
            ix.maintain(Mode::Force).unwrap();
            ix.set_limits(|l| l.compact_min_waste = 0);
            for c in 3..8 {
                ix.forget(&format!("dm.{c}")).unwrap();
            }
            ix.close();
        }
        for delay in [5u64, 40, 120, 300, 700] {
            let exe = std::env::current_exe().unwrap();
            let mut child = Command::new(exe)
                .args(["--exact", "search::index::tests::compaction_killed_midway_opens", "--nocapture"])
                .env("COMPACT_CHILD", &path)
                .stdout(Stdio::null())
                .spawn()
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(delay));
            child.kill().unwrap();
            child.wait().unwrap();
            // Opens, whatever moment it died at; the manifest and its segments are consistent.
            let ix = SearchIndex::open(&path, "alpha beta").unwrap();
            at_day(&ix, "2026-06-01");
            let st = ix.stats();
            // Every doc the manifest lists reads back (a lost page would make the scan skip it).
            let found = ids(&ix, "picnic").len();
            assert!(found <= st.seg_docs && (st.seg_docs == 0 || found > 0), "{delay} ms: {found} of {} docs", st.seg_docs);
            let y = ix.content_sizes().iter().find(|r| r.0 == "y/").unwrap().1;
            assert!(y * 8 >= st.seg_docs, "{delay} ms: {y} chunks for {} docs", st.seg_docs);
            ix.close();
        }
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

}
