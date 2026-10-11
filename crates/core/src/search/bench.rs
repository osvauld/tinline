//! Benchmark of the search index alone (no node, no network): `search-bench <messages>`.
//! Writes `n` synthetic messages one transaction each (the chat write path), rebuilds the same
//! corpus in per-day batches (the reconcile path), then times the query loop of
//! `Inner::search` (page, row, match, snippet) without the node-side name lookups.

use super::index::{DocRow, Mark, SearchIndex};
use super::text;
use std::path::Path;
use std::time::{Duration, Instant};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    /// Zipf-like: low indexes far more often.
    fn zipf(&mut self, n: usize) -> usize {
        let u = (self.next() % 1_000_000) as f64 / 1e6;
        ((n as f64).powf(u) - 1.0) as usize % n
    }
}

const HEAD: &[&str] = &[
    "the", "to", "and", "a", "of", "i", "you", "it", "is", "that", "in", "for", "on", "we", "this", "with", "have", "do", "not", "are", "but", "they", "what", "so", "can", "will", "at", "be", "me", "my", "just", "there", "then", "when", "meeting", "tomorrow", "tonight", "dinner", "photo", "call", "thanks", "okay", "message", "today", "later", "office", "weekend", "payment", "ticket", "flight",
];

fn vocab(rng: &mut Rng, n: usize, syll: &[&str]) -> Vec<String> {
    (0..n).map(|_| (0..2 + rng.below(3)).map(|_| syll[rng.below(syll.len())]).collect()).collect()
}

struct Corpus {
    en: Vec<String>,
    hi: Vec<String>,
    ml: Vec<String>,
    cjk: Vec<char>,
}

fn corpus(rng: &mut Rng) -> Corpus {
    let lat = ["ba", "ke", "ri", "mo", "tu", "sa", "ne", "li", "do", "pa", "ve", "gu", "shi", "ton", "ler", "ing", "ment", "tra", "con", "pre"];
    let hi = ["का", "की", "में", "है", "कर", "ना", "ता", "रा", "सी", "वह", "आप", "हम", "बात", "घर", "काम", "लोग"];
    let ml = ["ഒരു", "അത്", "ഇത്", "നാ", "കേ", "വീ", "ടു", "പോ", "ക്ക", "ല്ല", "ആണ്", "എന്ന", "ഞാൻ", "നീ"];
    let mut en: Vec<String> = HEAD.iter().map(|s| s.to_string()).collect();
    en.extend(vocab(rng, 8000, &lat));
    Corpus { en, hi: vocab(rng, 1500, &hi), ml: vocab(rng, 1500, &ml), cjk: (0..2500u32).filter_map(|i| char::from_u32(0x4e00 + i * 7)).collect() }
}

fn message(rng: &mut Rng, c: &Corpus) -> String {
    let words = 5 + rng.below(36);
    let pick = rng.below(100);
    let mut out = Vec::new();
    if pick < 72 {
        for _ in 0..words {
            out.push(c.en[rng.zipf(c.en.len())].clone());
        }
    } else if pick < 82 {
        for _ in 0..words {
            out.push(c.hi[rng.zipf(c.hi.len())].clone());
        }
    } else if pick < 88 {
        for _ in 0..words {
            out.push(c.ml[rng.zipf(c.ml.len())].clone());
        }
    } else if pick < 96 {
        // CJK runs have no spaces: about 2-3 chars a "word".
        for _ in 0..words {
            let n = 2 + rng.below(2);
            out.push((0..n).map(|_| c.cjk[rng.zipf(c.cjk.len())]).collect());
        }
        return out.concat();
    } else {
        for i in 0..words {
            out.push(if i % 3 == 0 { c.hi[rng.zipf(c.hi.len())].clone() } else { c.en[rng.zipf(c.en.len())].clone() });
        }
    }
    out.join(" ")
}

const CONVS: usize = 50;
const GROUPS: usize = 10;
const DAYS: usize = 730;
const END: i64 = 1_791_000_000_000;

fn rows(n: usize) -> Vec<DocRow> {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let c = corpus(&mut rng);
    let span = DAYS as i64 * 86_400_000;
    let mut at: Vec<i64> = (0..n).map(|_| END - span + (rng.next() % span as u64) as i64).collect();
    at.sort();
    at.into_iter()
        .enumerate()
        .map(|(i, at)| {
            let conv = rng.zipf(CONVS);
            let group = conv >= CONVS - GROUPS;
            let mut text = message(&mut rng, &c);
            if i % (n / 5).max(1) == 7 {
                text.push_str(" zyxqrare"); // the rare word: 5 messages
            }
            let day = format!("{}", at / 86_400_000);
            DocRow {
                src: if group { format!("g.{conv}") } else { format!("dm.{conv}") },
                conv: conv.to_string(),
                day: if group { "-".into() } else { day },
                id: format!("m{i:08}"),
                at,
                author: format!("did:{}", rng.below(8)),
                text,
            }
        })
        .collect()
}

fn pct(v: &mut [Duration], p: f64) -> Duration {
    v.sort();
    v[((v.len() as f64 * p) as usize).min(v.len() - 1)]
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn size(p: &Path) -> f64 {
    std::fs::metadata(p).map(|m| m.len() as f64 / 1048576.0).unwrap_or(0.0)
}

/// What `Inner::search` does per hit, minus names and conversation lookups.
fn query(ix: &SearchIndex, q: &str, limit: usize, cursor: Option<&str>) -> (usize, Option<String>) {
    let terms = text::query_terms(q);
    let scan = terms.iter().max_by_key(|t| t.weight()).unwrap();
    let lookup = scan.lookup();
    let (mut after, mut looked, mut hits, mut next) = (cursor.map(str::to_string), 0, 0, None);
    'scan: loop {
        let (page, more) = ix.page(&lookup, after.as_deref(), 256).unwrap();
        for (cur, row) in page {
            looked += 1;
            after = Some(cur.clone());
            let Some(row) = row else { continue };
            let n = text::normalize(&row.text);
            if let Some(h) = text::match_all(&terms, &n) {
                let _ = text::snippet(&row.text, &n, &h);
                hits += 1;
                if hits >= limit {
                    next = Some(cur);
                    break 'scan;
                }
            }
            if looked >= 20_000 {
                next = Some(cur);
                break 'scan;
            }
        }
        if !more {
            break;
        }
    }
    (hits, next)
}

fn lat(label: &str, mut v: Vec<Duration>) {
    println!("  {label:<34} p50 {:>8.3} ms  p95 {:>8.3} ms  p99 {:>8.3} ms  max {:>8.3} ms", ms(pct(&mut v, 0.5)), ms(pct(&mut v, 0.95)), ms(pct(&mut v, 0.99)), ms(*v.last().unwrap()));
}

pub fn run(n: usize, dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    let data = rows(n);
    println!("search-bench: {n} messages, {CONVS} conversations ({GROUPS} groups), {DAYS} days, {} cpus", std::thread::available_parallelism().map_or(1, |n| n.get()));

    // 1. Live write path: one transaction per message, as the chat hooks do.
    let path = dir.join("live.redb");
    let _ = std::fs::remove_file(&path);
    let ix = SearchIndex::open(&path, "bench phrase").unwrap();
    let mut each = Vec::with_capacity(n);
    let t0 = Instant::now();
    for (i, r) in data.iter().enumerate() {
        let mark = Mark { src: r.src.clone(), day: r.day.clone(), prev: None, vv: i.to_be_bytes().to_vec() };
        let t = Instant::now();
        ix.apply(std::slice::from_ref(r), &[], &[mark]).unwrap();
        each.push(t.elapsed());
        if (i + 1) % (n / 4).max(1) == 0 {
            println!("  live write {:>7} msgs: search.redb {:.1} MiB", i + 1, size(&path));
        }
    }
    println!("live write (one apply per message, incl. commit): {:.1} s total", t0.elapsed().as_secs_f64());
    lat("write latency per message", each);
    let live_size = size(&path);

    // 2. Rebuild: per (conversation, day) batches, groups a conversation at once.
    let rpath = dir.join("rebuild.redb");
    let _ = std::fs::remove_file(&rpath);
    let rix = SearchIndex::open(&rpath, "bench phrase").unwrap();
    let mut batches: std::collections::BTreeMap<(String, String), Vec<DocRow>> = Default::default();
    for r in &data {
        batches.entry((r.src.clone(), r.day.clone())).or_default().push(r.clone());
    }
    let nb = batches.len();
    let t0 = Instant::now();
    for ((src, day), b) in &batches {
        rix.apply(b, &[], &[Mark { src: src.clone(), day: day.clone(), prev: None, vv: vec![1] }]).unwrap();
    }
    let el = t0.elapsed().as_secs_f64();
    println!("rebuild ({nb} batches, avg {:.1} msgs): {el:.2} s = {:.0} msgs/s; search.redb {:.1} MiB", n as f64 / nb as f64, n as f64 / el, size(&rpath));

    // 2b. Rebuild in one batch per conversation (the group reconcile shape, and the best case).
    let wpath = dir.join("whole.redb");
    let _ = std::fs::remove_file(&wpath);
    let wix = SearchIndex::open(&wpath, "bench phrase").unwrap();
    let mut convs: std::collections::BTreeMap<String, Vec<DocRow>> = Default::default();
    for r in &data {
        convs.entry(r.conv.clone()).or_default().push(r.clone());
    }
    let t0 = Instant::now();
    for (c, b) in &convs {
        wix.apply(b, &[], &[Mark { src: b[0].src.clone(), day: format!("all{c}"), prev: None, vv: vec![1] }]).unwrap();
    }
    let el = t0.elapsed().as_secs_f64();
    println!("rebuild, one batch per conversation ({} batches): {el:.2} s = {:.0} msgs/s; search.redb {:.1} MiB", convs.len(), n as f64 / el, size(&wpath));

    // 2c. Where a live write goes: building the postings vs. the redb transaction.
    let k = data.len().min(2000);
    let t = Instant::now();
    let posts: usize = data[..k].iter().map(|r| text::index_terms(&r.text).iter().map(|w| ix.tok(w)).count()).sum();
    println!("postings per message: {:.0}; building them (terms + keyed hashes): {:.3} ms/msg", posts as f64 / k as f64, ms(t.elapsed()) / k as f64);
    let floor: Vec<Duration> = (0..200).map(|i| { let m = Mark { src: "floor".into(), day: "d".into(), prev: None, vv: vec![i as u8] }; let t = Instant::now(); ix.apply(&[], &[], &[m]).unwrap(); t.elapsed() }).collect();
    lat("commit floor (1 put, same file)", floor);

    // 3. Queries on the live index (page cache warm after the first run).
    println!("queries (limit 20, 30 runs, warm; first run cold-ish in brackets):");
    let cases = [("common prefix \"th\"", "th"), ("rare word \"zyxqrare\"", "zyxqrare"), ("2-word AND \"meeting tomorrow\"", "meeting tomorrow"), ("2-word AND \"the call\"", "the call"), ("no hit \"qqzzxx\"", "qqzzxx"), ("CJK 2 chars", &"")];
    let cjk_q: String = data.iter().find(|r| r.text.chars().next().is_some_and(text::is_bigram_script)).map(|r| r.text.chars().take(2).collect()).unwrap_or_default();
    for (label, q) in cases {
        let q = if label.starts_with("CJK") { cjk_q.as_str() } else { q };
        let t = Instant::now();
        let (h, _) = query(&ix, q, 20, None);
        let first = t.elapsed();
        let v: Vec<Duration> = (0..30).map(|_| { let t = Instant::now(); query(&ix, q, 20, None); t.elapsed() }).collect();
        print!("  [{:.2} ms, {h} hits] ", ms(first));
        lat(label, v);
    }
    // paging to page 5
    let mut v = Vec::new();
    let mut last = Vec::new();
    for _ in 0..30 {
        let (mut cur, t) = (None, Instant::now());
        let mut pages = Vec::new();
        for _ in 0..5 {
            let tp = Instant::now();
            let (_, nx) = query(&ix, "th", 20, cur.as_deref());
            pages.push(tp.elapsed());
            cur = nx;
        }
        v.push(t.elapsed());
        last.push(pages[4]);
    }
    lat("\"th\" pages 1-5 total", v);
    lat("\"th\" page 5 alone", last);
    println!("sizes: live {live_size:.1} MiB ({:.0} B/msg), rebuild {:.1} MiB ({:.0} B/msg)", live_size * 1048576.0 / n as f64, size(&rpath), size(&rpath) * 1048576.0 / n as f64);
}
