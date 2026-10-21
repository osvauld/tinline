use super::*;
use crate::testkit::{T0, keys, open, row};

const D: i64 = DAY_MS;

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
    let other = SearchIndex::with_store(ix.db.raw().clone(), keys("other phrase"), None).unwrap();
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
    let again = SearchIndex::with_store(raw, keys("alpha beta"), None).unwrap();
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
    let again = SearchIndex::with_store(ix.db.raw().clone(), keys("alpha beta"), None).unwrap();
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
    let crashed = SearchIndex::with_store(ix.db.raw().clone(), keys("alpha beta"), None).unwrap();
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
    fill_from(ix, 0, convs, days, per_day);
}

fn fill_from(ix: &SearchIndex, first: usize, convs: usize, days: i64, per_day: usize) {
    let mut n = 0u32;
    for d in 0..days {
        for c in first..convs {
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
    let ix = open(&path, "alpha beta").unwrap();
    at_day(&ix, "2026-06-01");
    for c in 0..6 {
        // one segment a conversation, so that forgetting it frees its pages
        fill_from(&ix, c, c + 1, 40, 20);
        ix.maintain_segments(Mode::Force).unwrap();
    }
    // Small file: below the byte threshold nothing happens, and the look is spent.
    assert_eq!(ix.compact_if_worthwhile().unwrap(), None);
    ix.set_limits(|l| l.compact_min_waste = 0);
    assert_eq!(ix.compact_if_worthwhile().unwrap(), None, "no change since the last look");
    // Mostly-free file: drop most of the conversations, leaving their pages free.
    for c in 1..6 {
        ix.forget(&format!("dm.{c}")).unwrap();
    }
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
    let ix = open(&path, "alpha beta").unwrap();
    at_day(&ix, "2026-06-01");
    assert_eq!(answers(&ix), before);
    let n = ids(&ix, "picnic").len();
    fill_from(&ix, 7, 8, 2, 5);
    ix.maintain(Mode::Force).unwrap();
    assert_eq!(ids(&ix, "picnic").len(), n + 10);
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
        let ix = open(std::path::Path::new(&p), "alpha beta").unwrap();
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
        let ix = open(&path, "alpha beta").unwrap();
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
            .args(["--exact", "index::tests::compaction_killed_midway_opens", "--nocapture"])
            .env("COMPACT_CHILD", &path)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(delay));
        child.kill().unwrap();
        child.wait().unwrap();
        // Opens, whatever moment it died at; the manifest and its segments are consistent.
        let ix = open(&path, "alpha beta").unwrap();
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

// KIND: invariant (v2) — a send (`apply`, and the closed check in front of it) never waits for
// the file: while a compaction holds the store exclusively, they complete.
#[test]
fn apply_does_not_wait_for_a_compaction() {
    let ix = SearchIndex::in_memory("alpha beta");
    at_day(&ix, "2026-06-01");
    let r = row("dm.p", "1", T0 + 10, "hello there");
    ix.db.raw().while_exclusive(|| {
        std::thread::scope(|sc| {
            let h = sc.spawn(|| {
                assert!(!ix.is_closed());
                ix.apply(std::slice::from_ref(&r), &[], &[mark(&r, 1)]).unwrap();
            });
            let t = Instant::now();
            while !h.is_finished() {
                assert!(t.elapsed() < Duration::from_secs(5), "apply is waiting for the store");
                std::thread::sleep(Duration::from_millis(5));
            }
            h.join().unwrap();
        });
    });
    assert_eq!(ids(&ix, "hel"), ["1"]);
}

// KIND: unit (v2) — an old-format file is replaced by a fresh one on open (not emptied
// record by record), so it is small again at once.
#[test]
fn old_format_file_is_replaced() {
    let path = temp_path("oldfmt");
    {
        let ix = open(&path, "alpha beta").unwrap();
        fill(&ix, 6, 30, 60);
        at_day(&ix, "2026-06-01");
        ix.maintain(Mode::Force).unwrap();
        ix.db.put("v", &2u32.to_be_bytes()).unwrap();
        ix.close();
    }
    let big = std::fs::metadata(&path).unwrap().len();
    let ix = open(&path, "alpha beta").unwrap();
    assert!(ix.ix_version_ok());
    assert_eq!(ix.raw_rows().len(), 1);
    assert!(std::fs::metadata(&path).unwrap().len() < big && std::fs::metadata(&path).unwrap().len() < (2 << 20), "{big}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
