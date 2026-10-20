//! The lazy newest-first walk over a term's postings (hot tier and segments).

use super::*;

pub(super) type Chunk = Arc<Vec<(u64, DocRow)>>;

pub(super) struct Open {
    seg: u32,
    bbits: u8,
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
    pub(super) ix: &'a SearchIndex,
    pub(super) toks: Vec<u64>,
    pub(super) view: Arc<View>,
    pub(super) hot_days: HashSet<DayKey>,
    pub(super) after: Option<(u64, u64)>,
    pub(super) hot_buf: VecDeque<(u64, u64, DocRow)>,
    pub(super) hot_last: Option<(u64, u64)>,
    pub(super) hot_done: bool,
    pub(super) pending: VecDeque<Seg>,
    pub(super) open: Vec<Open>,
    pub(super) chunks: HashMap<(u32, usize), Option<Chunk>>,
    pub(super) buckets: HashMap<(u32, u32), Option<Vec<u8>>>,
    pub(super) last: Option<(u64, u64)>,
    pub(super) filt: Option<Filter>,
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

    /// Find in chat: the cursor to continue from when the drop budget ended the walk.
    pub fn stopped(&self) -> Option<String> {
        self.filt.as_ref().and_then(|f| f.stopped.clone())
    }

    /// Find in chat: (segments opened, postings dropped by the doc table, doc table reads).
    pub fn find_stats(&self) -> (usize, usize, usize) {
        self.filt.as_ref().map_or((0, 0, 0), |f| (f.opened, f.dropped, f.z_reads))
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
        let v = self.ix.hot.lock().range(self.toks[0], &self.toks[1..], self.hot_last, self.filt.as_ref().map(|f| f.src), HOT_BATCH);
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
        if let Some(f) = &mut self.filt {
            f.open(s);
        }
        self.open.push(Open { seg: s.id, bbits: s.bbits, docs: s.docs, part, list: Vec::new(), pos: 0, head: None });
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
            let bbits = self.open[i].bbits;
            for tok in self.toks.clone() {
                let bk = (seg, bucket_of(tok, bbits));
                let ix = self.ix;
                let bucket = self.buckets.entry(bk).or_insert_with(|| ix.db.get(&b_key(bk.0, bk.1)).ok().flatten());
                let short = bucket.as_deref().and_then(|b| bucket_find(b, tok, part));
                let l = match short {
                    Some(l) => l,
                    None => self.ix.db.get(&x_key(seg, tok, part)).ok().flatten().and_then(|b| decode_list(&b, (part * PART_DOCS) as u32)).unwrap_or_default(),
                };
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
            if let Some(f) = &mut self.filt {
                let ix = self.ix;
                match f.check(seg, doc, &mut |p| ix.read_z(seg, p)) {
                    Verdict::Keep => {}
                    Verdict::Drop => {
                        self.open[i].pos += 1;
                        continue;
                    }
                    Verdict::Stop => {
                        // Budget spent: a pseudo-posting at this doc ends the page when the
                        // merge reaches it (the row is read for its cursor only).
                        if let Some((h, row)) = self.row(seg, doc) {
                            let key = (rev_at(row.at), h);
                            if let Some(f) = &mut self.filt {
                                f.stops.insert(key);
                            }
                            self.open[i].head = Some((key.0, key.1, row));
                            return;
                        }
                        self.open[i].pos += 1;
                        continue;
                    }
                }
            }
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
            if let Some(f) = &mut self.filt {
                if seg.is_some() && f.stops.contains(&(rev, h)) {
                    f.stopped = Some(format!("{rev:016x}/{h:016x}"));
                    return None;
                }
                if seg.is_some() && self.ix.keyer.src(&row.src) != f.src {
                    continue;
                }
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
