use super::super::index::{DocRow, Mode, SearchIndex, VERSION};
use super::*;

const DAY: i64 = 86_400_000;
/// 2026-01-01 00:00 UTC.
const T0: i64 = 1_767_225_600_000;

fn row(src: &str, id: &str, at: i64, text: &str) -> DocRow {
    DocRow { src: src.into(), conv: "did:key:z".into(), day: crate::chat::doc::day_of(at), id: id.into(), at, author: "did:key:a".into(), text: text.into() }
}

/// Everything cold (today is far ahead).
fn cold(ix: &SearchIndex) {
    ix.set_today(Some("2027-06-01"));
}

fn walk(ix: &SearchIndex, term: &str, src: &str, drop_budget: usize) -> (Vec<String>, Option<String>, (usize, usize, usize)) {
    let mut scan = ix.scan_in(&[term.to_string()], None, src, drop_budget).unwrap();
    let mut out = Vec::new();
    loop {
        let (page, more) = scan.next(64).unwrap();
        out.extend(page.into_iter().map(|(_, r)| r.id));
        if !more {
            break;
        }
    }
    let stats = scan.find_stats();
    (out, scan.stopped(), stats)
}

// KIND: unit (find in chat) — a conversation's postings only, newest first, from the hot
// tier and the segments; segments without its days are not opened; the doc table drops the
// others' postings (counted) without them showing up.
#[test]
fn scan_in_walks_one_conversation() {
    let ix = SearchIndex::in_memory("alpha beta");
    // January: only dm.b (200 docs). February: dm.a (3) amid dm.b (300), same days. Then
    // the hot tier: dm.a 1, dm.b 5.
    let mut rows = Vec::new();
    for i in 0..200 {
        rows.push(row("dm.b", &format!("jb{i}"), T0 + i * 1000, "common prefix chatter"));
    }
    let feb = T0 + 40 * DAY;
    for i in 0..300 {
        rows.push(row("dm.b", &format!("fb{i}"), feb + i * 1000, "common prefix chatter"));
    }
    for i in 0..3 {
        rows.push(row("dm.a", &format!("fa{i}"), feb + i * 100_000 + 500, "common quiet words"));
    }
    ix.set_today(Some("2026-01-05"));
    ix.apply_bulk(&rows, &[], &[]).unwrap();
    cold(&ix);
    ix.maintain(Mode::Force).unwrap();
    assert!(ix.stats().segs >= 2);
    // Hot rows: today is mid-way through; make them recent relative to the new clock.
    let hot_at = 1_780_272_000_000; // 2026-06-01
    ix.apply(&[row("dm.a", "ha", hot_at, "common hot quiet"), row("dm.b", "hb", hot_at + 1, "common hot chatter")], &[], &[]).unwrap();
    ix.set_today(Some("2026-06-02"));
    let (got, stopped, (opened, dropped, z_reads)) = walk(&ix, "comm", "dm.a", 1000);
    assert_eq!(got, ["ha", "fa2", "fa1", "fa0"]);
    assert_eq!(stopped, None);
    assert_eq!(opened, 1, "the January segment has no dm.a day");
    assert_eq!(dropped, 300, "dm.b's February postings are dropped by the doc table");
    assert!(z_reads >= 1);
    // The other conversation is unaffected.
    let (b, _, (opened_b, dropped_b, _)) = walk(&ix, "comm", "dm.b", 1000);
    assert_eq!(b.len(), 501);
    assert_eq!(opened_b, 2);
    assert_eq!(dropped_b, 3);
    // A conversation nobody has: nothing, and no segment opened.
    let (none, _, (opened_n, _, _)) = walk(&ix, "comm", "dm.zzz", 1000);
    assert!(none.is_empty());
    assert_eq!(opened_n, 0);
}

// KIND: unit (find in chat) — the drop budget ends a page with a cursor that continues
// exactly where it stopped: every posting of the conversation once, newest first.
#[test]
fn drop_budget_stops_and_continues() {
    let ix = SearchIndex::in_memory("alpha beta");
    let mut rows = Vec::new();
    for i in 0..600 {
        rows.push(row("dm.b", &format!("b{i}"), T0 + i * 1000, "common prefix chatter"));
        if i % 150 == 7 {
            rows.push(row("dm.a", &format!("a{i}"), T0 + i * 1000 + 500, "common quiet"));
        }
    }
    ix.set_today(Some("2026-01-05"));
    ix.apply_bulk(&rows, &[], &[]).unwrap();
    cold(&ix);
    ix.maintain(Mode::Force).unwrap();
    let want: Vec<String> = (0..600).rev().filter(|i| i % 150 == 7).map(|i| format!("a{i}")).collect();
    assert_eq!(walk(&ix, "common", "dm.a", 10_000).0, want);
    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    let mut pages = 0;
    loop {
        let mut scan = ix.scan_in(&["common".to_string()], cursor.as_deref(), "dm.a", 100).unwrap();
        loop {
            let (page, more) = scan.next(64).unwrap();
            seen.extend(page.into_iter().map(|(_, r)| r.id));
            if !more {
                break;
            }
        }
        pages += 1;
        cursor = scan.stopped();
        if cursor.is_none() {
            break;
        }
        assert!(pages < 20);
    }
    assert_eq!(seen, want, "no gaps, no repeats");
    assert!(pages >= 3, "a budget of 100 dropped postings cuts the walk: {pages}");
}

// KIND: unit (find in chat) — a file of format 2 (no doc tables; any other version alike) is emptied on open.
#[test]
fn format_2_file_is_emptied() {
    let ix = SearchIndex::in_memory("alpha beta");
    let raw = ix.db.raw().clone();
    ix.db.put("v", &2u32.to_be_bytes()).unwrap();
    ix.db.put("m", b"old manifest").unwrap();
    ix.db.put("x/00000001/00112233/000", b"old").unwrap();
    let again = SearchIndex::with_store_for_test(raw);
    assert_eq!(again.raw_rows().len(), 1, "only the version row");
    assert_eq!(VERSION, 4);
    assert_eq!(again.db.get("v").unwrap().unwrap(), VERSION.to_be_bytes());
}

// KIND: measurement (find in chat) — a small chat among 100k messages of a common prefix.
// Run: cargo test --release -p p2pcore --lib find_in_chat_latency -- --ignored --nocapture
#[test]
#[ignore]
fn find_in_chat_latency_100k() {
    let ix = SearchIndex::in_memory("alpha beta");
    let t = std::time::Instant::now();
    let mut rows = Vec::new();
    // 14 months of days; 2 000 a day, a quiet chat (dm.q) with a message every 500th.
    for i in 0..100_000i64 {
        let at = T0 + (i / 250) * DAY + (i % 250) * 1000;
        let src = if i % 500 == 3 { "dm.q" } else { "dm.big" };
        rows.push(row(src, &format!("m{i}"), at, &format!("common prefix chatter number {i}")));
    }
    ix.set_today(Some("2026-01-05"));
    ix.apply_bulk(&rows, &[], &[]).unwrap();
    ix.set_today(Some("2028-01-01"));
    ix.maintain(Mode::Force).unwrap();
    println!("built in {:?}: {:?}", t.elapsed(), ix.stats());
    for (src, label, want) in [("dm.q", "quiet chat (200 msgs), first 20", 20), ("dm.q", "quiet chat, all 200 (whole 100k scanned)", 100_000), ("dm.big", "busy chat, first 20", 20)] {
        let mut times = Vec::new();
        let mut found = 0;
        for _ in 0..5 {
            let t = std::time::Instant::now();
            let mut scan = ix.scan_in(&["common".to_string()], None, src, DROP_BUDGET).unwrap();
            let (page, _) = scan.next(want).unwrap();
            found = page.len();
            times.push(t.elapsed());
        }
        times.sort();
        println!("{label}: {found} hits, median {:?}, worst {:?}, stats {:?}", times[2], times[4], ix.scan_in(&["common".to_string()], None, src, DROP_BUDGET).map(|s| s.find_stats()).unwrap());
    }
    // Global search for comparison, 20 hits.
    let t = std::time::Instant::now();
    let mut scan = ix.scan_all(&["common".to_string()], None).unwrap();
    let (page, _) = scan.next(20).unwrap();
    println!("global first 20 ({}): {:?}", page.len(), t.elapsed());
}
