//! Writing cold days into segments, merging a month's segments, and compacting the file.

use super::*;

impl SearchIndex {
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
    /// [`COMPACT_MIN_WASTE`] bytes and more than a third of the file (segment merges and rewrites
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
        if u.wasted() <= self.limits().compact_min_waste || u.wasted() * 3 <= u.file_bytes {
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

    #[doc(hidden)]
    pub fn maintain_segments(&self, mode: Mode) -> Result<(), Error> {
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
            // Half the cores, at low priority: a send must never wait for a core.
            let workers = (std::thread::available_parallelism().map_or(1, |n| n.get()) / 2).max(1).min(jobs.len()).max(1);
            std::thread::scope(|sc| {
                for _ in 0..workers {
                    sc.spawn(|| {
                        background_thread();
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
            ops.push(Op::DeletePrefix(format!("z/{id:08x}/")));
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
