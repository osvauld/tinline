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
use storage::{Op, Sealed, Store};

use super::find::{Filter, Verdict};
use super::hot::Hot;
use super::seg::{self, Built, CHUNK, DayData, PART_DOCS, DayKey, Digest, Manifest, NO_DIGEST, Seg, View, decode_chunk, decode_list, rev_at, x_key, y_key, z_key, b_key, bucket_find, bucket_of};
use crate::Error;
use crdt::{DAY_MS, day_of, day_start};

/// Bump when the tokenizer or the layout changes: the index is rebuilt from the chat and group
/// stores. (1: per-message postings in the file. 2: hot tier + segments. 3: bucketed short
/// posting lists. 4: `z/` doc tables for find in chat.)
pub const VERSION: u32 = 4;
/// The key domain of the store's sealing key (p2pcore derives [`Keys::store`] under it).
pub const STORE_DOMAIN: &str = "tinline/search-store/v1";
/// The key domain of the keyed doc/term/conversation hashes ([`Keys::index`]).
pub const INDEX_DOMAIN: &str = "tinline/search-index/v1";

/// The two keys of an index, derived by the caller from the account's phrase under
/// [`STORE_DOMAIN`] and [`INDEX_DOMAIN`].
#[derive(Clone)]
pub struct Keys {
    /// Seals every record of the file.
    pub store: cryptography::Secret32,
    /// Keys the doc, term, conversation and day hashes.
    pub index: cryptography::Secret32,
}

/// Opens (creating) the store file at a path: p2pcore waits for a previous owner and makes the
/// file private.
pub type OpenStore<'a> = &'a dyn Fn(&Path) -> Result<Store, Error>;

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
pub struct Keyer(cryptography::Secret32);

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
/// Lowers the priority of the calling thread (segment builders must not take the core a send
/// is running on from it).
fn background_thread() {
    #[cfg(unix)]
    // SAFETY: plain syscalls on the calling thread's own id; failure is ignored.
    unsafe {
        let tid = libc::syscall(libc::SYS_gettid) as libc::id_t;
        libc::setpriority(libc::PRIO_PROCESS, tid, 15);
    }
}

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
    pub(super) db: Sealed,
    keyer: Keyer,
    hot: Mutex<Hot>,
    view: RwLock<Arc<View>>,
    maint: Mutex<Memo>,
    lim: RwLock<Limits>,
    today: Mutex<Option<u32>>,
    ticker: std::sync::atomic::AtomicBool,
    /// Segments were written or dropped since the last look at the file's free space.
    file_changed: std::sync::atomic::AtomicBool,
    /// `close` was called (the hot tier is wiped; `apply` ignores changes).
    closed: std::sync::atomic::AtomicBool,
}

impl SearchIndex {
    /// Opens (creating) the index; a file of another format version is emptied (a rebuild).
    pub fn open(path: &Path, keys: Keys, open_store: OpenStore<'_>) -> Result<Self, Error> {
        let store = open_store(path)?;
        Self::with_store(store, keys, Some((path, open_store)))
    }

    #[cfg(test)]
    pub fn with_store_for_test(store: Store) -> Self {
        Self::with_store(store, crate::testkit::keys("alpha beta"), None).unwrap()
    }

    #[cfg(test)]
    pub fn in_memory(phrase: &str) -> Self {
        Self::with_store(Store::open_in_memory().unwrap(), crate::testkit::keys(phrase), None).unwrap()
    }

    fn with_store(store: Store, keys: Keys, file: Option<(&Path, OpenStore<'_>)>) -> Result<Self, Error> {
        let mut me = Self {
            db: Sealed::new(store, keys.store.clone()),
            keyer: Keyer(keys.index),
            hot: Mutex::new(Hot::default()),
            view: RwLock::new(Arc::new(View::default())),
            maint: Mutex::new(Memo::default()),
            lim: RwLock::new(Limits::default()),
            today: Mutex::new(None),
            ticker: std::sync::atomic::AtomicBool::new(false),
            file_changed: std::sync::atomic::AtomicBool::new(true),
            closed: std::sync::atomic::AtomicBool::new(false),
        };
        let current = matches!(me.db.get("v"), Ok(Some(v)) if v == VERSION.to_be_bytes());
        let man = if current { me.db.get("m").ok().flatten().and_then(|b| cryptography::codec::from_bytes::<Manifest>(&b, storage::MAX_ROW).ok()) } else { None };
        match man {
            Some(m) => *me.view.write() = Arc::new(View::new(m)),
            None => {
                // Another version, no manifest or an unreadable one: start over. A file is
                // replaced by a new one (deleting a gigabyte of records one by one takes
                // half a minute, and the file would stay that big).
                if let Some((path, open_store)) = file {
                    me.db.close();
                    let _ = std::fs::remove_file(path);
                    let store = open_store(path)?;
                    me.db = Sealed::new(store, keys.store);
                }
                me.db.apply(&[Op::DeletePrefix(String::new()), me.db.put_op("v", &VERSION.to_be_bytes())?])?;
            }
        }
        Ok(me)
    }

    /// Lets the file go for every holder (stop of the account's stores) and forgets the hot
    /// tier.
    pub fn close(&self) {
        self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
        self.db.close();
        self.hot.lock().wipe();
        *self.view.write() = Arc::new(View::default());
    }

    /// Whether `close` was called. A flag, not the store's lock: that one is held by a
    /// compaction, and the send path asks this.
    pub fn is_closed(&self) -> bool {
        self.closed.load(std::sync::atomic::Ordering::SeqCst)
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
            let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64) as i64;
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

    fn read_z(&self, seg: u32, part: usize) -> Option<Vec<u8>> {
        self.db.get(&z_key(seg, part)).ok().flatten()
    }

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
                let changes = puts.iter().any(|(h, k, r)| *k == key && (crate::text::index_terms(&r.text).is_empty() == have.contains_key(h) || (have.get(h).is_some_and(|o| *o != *r))))
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
            ops.push(Op::DeletePrefix(format!("z/{:08x}/", g.id)));
        }
        man.segs.retain(|g| !g.days.is_empty());
        ops.push(self.db.put_op("m", &cryptography::codec::to_bytes(&man).expect("serialises"))?);
        self.db.apply(&ops)?;
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
        self.scan_filtered(terms, after, None)
    }

    /// [`scan_all`](Self::scan_all) over one conversation's docs only (find in chat).
    pub fn scan_in(&self, terms: &[String], after: Option<&str>, src: &str, drop_budget: usize) -> Result<Scan<'_>, Error> {
        self.scan_filtered(terms, after, Some(Filter::new(self.keyer.src(src), drop_budget)))
    }

    fn scan_filtered(&self, terms: &[String], after: Option<&str>, filt: Option<Filter>) -> Result<Scan<'_>, Error> {
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
        let mut pending: Vec<Seg> = view.man.segs.iter().filter(|s| s.docs > 0 && after.is_none_or(|a| rev_at(s.min_at) >= a.0) && filt.as_ref().is_none_or(|f| f.wants(&view, s))).cloned().collect();
        pending.sort_by_key(|s| (std::cmp::Reverse(s.max_at), std::cmp::Reverse(s.id)));
        Ok(Scan { ix: self, toks, view, hot_days, after, hot_buf: VecDeque::new(), hot_last: after, hot_done: false, pending: pending.into(), open: Vec::new(), chunks: HashMap::new(), buckets: HashMap::new(), last: None, filt })
    }

    /// `(records, key bytes + value bytes)` of the `x/`, `y/` and `m` records in the file.
    #[cfg(any(test, feature = "bench"))]
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

mod maintenance;
mod scan;
pub use scan::Scan;

#[cfg(test)]
mod tests;
