//! One heavy year of chat data: generate it once (`gen`), then measure the core on copies of it
//! (`run`). See `docs/scale-year.md`.
//!
//!   year-bench gen --scale 1.0 --dir /home/abe/build-cache/search-year-data/s1.0
//!   year-bench run --dir <same> --scenario intact|rebuild|bg [--work <dir>]
//!
//! Desktop only; no network is used (the node is never started).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use p2pcore::{CallInfo, CallState, Node, NodeEvents, NodeStatus};
use serde_json::{Value, json};

struct Quiet;
impl NodeEvents for Quiet {
    fn on_status(&self, _: NodeStatus) {}
    fn on_contacts_changed(&self) {}
    fn on_incoming_call(&self, _: CallInfo) {}
    fn on_call_state(&self, _: String, _: CallState) {}
    fn on_log(&self, _: String) {}
}

const PASS: &str = "bench-passphrase";
const DAY_MS: i64 = 86_400_000;
const DAYS: i64 = 365;
const CONTACTS: usize = 40;
const POOL: usize = 130;

// ---- small helpers -----------------------------------------------------------------------------

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
    fn unit(&mut self) -> f64 {
        (self.next() % 1_000_000) as f64 / 1e6
    }
    /// Zipf-like: low indexes far more often.
    fn zipf(&mut self, n: usize) -> usize {
        ((n as f64).powf(self.unit()) - 1.0) as usize % n
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

fn day_of(ms: i64) -> String {
    let z = ms.div_euclid(DAY_MS) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{:02}-{:02}", y + (m <= 2) as i64, m, d)
}

fn rss_kb(field: &str) -> u64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    s.lines().find(|l| l.starts_with(field)).and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse().ok()).unwrap_or(0)
}
fn rss_mb() -> f64 {
    rss_kb("VmRSS:") as f64 / 1024.0
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

struct Stat(Vec<f64>);
impl Stat {
    fn p(&self, q: f64) -> f64 {
        let mut v = self.0.clone();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        if v.is_empty() { 0.0 } else { v[((v.len() as f64 * q) as usize).min(v.len() - 1)] }
    }
    fn json(&self) -> Value {
        let max = self.0.iter().cloned().fold(0.0, f64::max);
        json!({"n": self.0.len(), "p50_ms": round(self.p(0.5)), "p95_ms": round(self.p(0.95)), "p99_ms": round(self.p(0.99)), "max_ms": round(max)})
    }
}
fn round(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

fn timed<T>(f: impl FnOnce() -> T) -> (T, f64) {
    let t = Instant::now();
    let r = f();
    (r, ms(t.elapsed()))
}

fn dir_size(p: &Path) -> u64 {
    let Ok(md) = std::fs::symlink_metadata(p) else { return 0 };
    if md.is_file() {
        return md.len();
    }
    std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| dir_size(&e.path())).sum()
}

fn account_dir(root: &Path) -> PathBuf {
    let d = root.join("accounts");
    std::fs::read_dir(d).unwrap().flatten().next().expect("an account dir").path()
}

fn sizes(root: &Path) -> Value {
    let a = account_dir(root);
    let mb = |n: &str| round(dir_size(&a.join(n)) as f64 / 1048576.0);
    json!({"chat.redb_mb": mb("chat.redb"), "group.redb_mb": mb("group.redb"), "blobs_mb": mb("blobs"), "search.redb_mb": mb("search.redb"), "account_total_mb": round(dir_size(&a) as f64 / 1048576.0)})
}

// ---- text --------------------------------------------------------------------------------------

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

fn corpus() -> Corpus {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let lat = ["ba", "ke", "ri", "mo", "tu", "sa", "ne", "li", "do", "pa", "ve", "gu", "shi", "ton", "ler", "ing", "ment", "tra", "con", "pre"];
    let hi = ["का", "की", "में", "है", "कर", "ना", "ता", "रा", "सी", "वह", "आप", "हम", "बात", "घर", "काम", "लोग"];
    let ml = ["ഒരു", "അത്", "ഇത്", "നാ", "കേ", "വീ", "ടു", "പോ", "ക്ക", "ല്ല", "ആണ്", "എന്ന", "ഞാൻ", "നീ"];
    let mut en: Vec<String> = HEAD.iter().map(|s| s.to_string()).collect();
    en.extend(vocab(&mut rng, 8000, &lat));
    Corpus { en, hi: vocab(&mut rng, 1500, &hi), ml: vocab(&mut rng, 1500, &ml), cjk: (0..2500u32).filter_map(|i| char::from_u32(0x4e00 + i * 7)).collect() }
}

/// A chat-sized message: mean ~10 words, 72% English-like, the rest Hindi, Malayalam, CJK, mixed.
fn message(rng: &mut Rng, c: &Corpus) -> String {
    let words = (2.0 + -rng.unit().max(1e-6).ln() * 8.0).min(60.0) as usize;
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

// ---- gen ---------------------------------------------------------------------------------------

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn generate(args: &[String]) {
    let scale: f64 = arg(args, "--scale").map_or(1.0, |s| s.parse().unwrap());
    let dir = PathBuf::from(arg(args, "--dir").expect("--dir"));
    let _ = std::fs::remove_dir_all(&dir);
    let root = dir.join("root");
    std::fs::create_dir_all(&root).unwrap();
    let t0 = Instant::now();
    let node = Node::new(root.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
    let phrase = node.create_identity("Bench Me".into(), PASS.into()).unwrap();
    let me = node.profile().unwrap().did;
    let key = node.unlock_key().expect("unlock key");
    let c = corpus();
    let mut rng = Rng(0x1234_5678_9abc_def1);

    // Peers: contacts first.
    let mut pool: Vec<(String, String)> = Vec::new(); // (phrase, did)
    for i in 0..POOL {
        let ph = node.synth_phrase_for_test();
        let did = node.synth_peer_for_test(ph.clone()).unwrap();
        if i < CONTACTS {
            node.synth_contact_for_test(did.clone(), format!("Contact {i:02}")).unwrap();
        }
        pool.push((ph, did));
    }
    eprintln!("peers ready {:.1}s", t0.elapsed().as_secs_f64());

    // DMs.
    let end = now_ms();
    let today_start = end.div_euclid(DAY_MS) * DAY_MS;
    let per_day = 60.0 * scale;
    let rare = ["zyxqrare", "qwertyuniq"];
    let mut total = 0u64;
    let mut files = 0u64;
    let mut n_rare = 0usize;
    let t = Instant::now();
    for d in 0..DAYS {
        let day_start = today_start - (DAYS - 1 - d) * DAY_MS;
        let span = if d == DAYS - 1 { (end - day_start).max(1) } else { DAY_MS };
        let n = (per_day * (0.4 + 1.2 * rng.unit())).round() as usize;
        let mut ats: Vec<i64> = (0..n).map(|_| day_start + (rng.next() % span as u64) as i64).collect();
        ats.sort();
        for at in ats {
            let who = rng.zipf(CONTACTS);
            let author = if rng.below(2) == 0 { me.clone() } else { pool[who].1.clone() };
            let mut text = message(&mut rng, &c);
            if total % 4403 == 17 {
                text.push(' ');
                text.push_str(rare[n_rare % 2]);
                n_rare += 1;
            }
            let file = (total % 400 == 5).then(|| {
                files += 1;
                (format!("IMG_{total:06}.jpg"), 200_000 + rng.below(3_000_000) as u64)
            });
            node.synth_dm_for_test(pool[who].1.clone(), author, text, at, file).unwrap();
            total += 1;
        }
        if d % 30 == 29 {
            let closed = node.compact_now_for_test().unwrap();
            eprintln!("day {}/{DAYS}: {total} dms, {closed} days closed, {:.0}s, rss {:.0} MB", d + 1, t.elapsed().as_secs_f64(), rss_mb());
        }
    }
    let closed = node.compact_now_for_test().unwrap();
    eprintln!("dms done: {total} msgs ({files} with file), {closed} shard-days closed, {:.0}s", t.elapsed().as_secs_f64());

    // Groups.
    let specs: [(&str, usize, usize); 5] = [("Family", 2000, 6), ("Neighbours", 2000, 7), ("Trip 2026", 2000, 8), ("Office", 30_000, 25), ("Community", 100_000, 80)];
    let mut groups = Vec::new();
    for (gi, (name, posts, members)) in specs.iter().enumerate() {
        let t = Instant::now();
        let posts = ((*posts as f64) * scale).round() as usize;
        let mut g = node.synth_group_new_for_test(name.to_string()).unwrap();
        let start = today_start - (DAYS - 1) * DAY_MS;
        let mut ids = vec![(None::<String>, me.clone())];
        for k in 0..*members {
            let (_, did) = &pool[(gi * 7 + k) % POOL];
            node.synth_group_add_for_test(&mut g, did.clone(), format!("Member {k}"), start + k as i64).unwrap();
            ids.push((Some(did.clone()), did.clone()));
        }
        node.synth_group_flush_for_test(&mut g).unwrap();
        let mut ats: Vec<i64> = (0..posts).map(|_| start + DAY_MS + (rng.next() % ((DAYS - 1) * DAY_MS) as u64) as i64).collect();
        ats.sort();
        let batch = if posts > 20_000 { 5000 } else { 1000 };
        for (i, at) in ats.into_iter().enumerate() {
            let idx = rng.zipf(members + 1);
            let mut text = message(&mut rng, &c);
            if i % 2503 == 9 {
                text.push(' ');
                text.push_str(rare[(i / 2503) % 2]);
            }
            node.synth_group_post_for_test(&mut g, idx, at, text).unwrap();
            if (i + 1) % batch == 0 {
                node.synth_group_flush_for_test(&mut g).unwrap();
            }
        }
        node.synth_group_flush_for_test(&mut g).unwrap();
        eprintln!("group {name}: {posts} posts, {:.0}s, rss {:.0} MB", t.elapsed().as_secs_f64(), rss_mb());
        let mem: Vec<Value> = (1..=*members).map(|k| json!({"did": ids[k].1, "phrase": pool[(gi * 7 + k - 1) % POOL].0})).collect();
        groups.push(json!({"gid": g.gid(), "name": name, "posts": posts, "members": mem}));
    }
    let contacts: Vec<Value> = (0..CONTACTS).map(|i| json!({"did": pool[i].1, "phrase": pool[i].0})).collect();
    let manifest = json!({"scale": scale, "passphrase": PASS, "phrase": phrase, "did": me, "unlock_key": hex_of(&key), "end_ms": end, "days": DAYS, "dms": total, "dm_files": files, "contacts": contacts, "groups": groups, "generated_secs": t0.elapsed().as_secs_f64()});
    std::fs::write(dir.join("manifest.json"), serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    eprintln!("sizes: {}", sizes(&root));
    node.lock();
    std::thread::sleep(Duration::from_secs(3));
    eprintln!("generated in {:.0}s, peak rss {:.0} MB", t0.elapsed().as_secs_f64(), rss_kb("VmHWM:") as f64 / 1024.0);
}

fn hex_of(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
}

// ---- run ---------------------------------------------------------------------------------------

fn run(args: &[String]) {
    let dir = PathBuf::from(arg(args, "--dir").expect("--dir"));
    let scenario = arg(args, "--scenario").unwrap_or_else(|| "intact".into());
    let work = PathBuf::from(arg(args, "--work").unwrap_or_else(|| format!("{}-work-{scenario}", dir.display())));
    let m: Value = serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let (st, copy_ms) = timed(|| std::process::Command::new("cp").args(["-a", "--reflink=auto"]).arg(dir.join("root")).arg(work.join("root")).status().unwrap());
    assert!(st.success());
    // Let the copy's write-back finish, so it does not compete with the first reads.
    let _ = std::process::Command::new("sync").status();
    let root = work.join("root");
    let mut out = serde_json::Map::new();
    out.insert("scenario".into(), json!(scenario));
    out.insert("scale".into(), m["scale"].clone());
    out.insert("copy_ms".into(), json!(round(copy_ms)));
    out.insert("disk_before".into(), sizes(&root));
    if scenario == "rebuild" {
        let _ = std::fs::remove_file(account_dir(&root).join("search.redb"));
    }
    // A search.redb of the current format built by an earlier run replaces the dataset's own.
    if let Some(f) = arg(args, "--search-file") {
        std::fs::copy(&f, account_dir(&root).join("search.redb")).unwrap();
    }
    if scenario == "dirty" {
        // A process that is killed after a few writes: no store is closed cleanly.
        let first = m["contacts"][0]["did"].as_str().unwrap();
        let gid = m["groups"][0]["gid"].as_str().unwrap();
        let st = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["dirtywrite", &root.to_string_lossy(), m["unlock_key"].as_str().unwrap(), first, gid])
            .status()
            .unwrap();
        out.insert("dirty_writer_status".into(), json!(st.code()));
    }

    let rss0 = rss_mb();
    let (node, new_ms) = timed(|| Node::new(root.to_string_lossy().into(), Arc::new(Quiet)).unwrap());
    node.search_background_reconcile_for_test(scenario == "bg");
    let did = m["did"].as_str().unwrap().to_string();
    let contacts: Vec<String> = m["contacts"].as_array().unwrap().iter().map(|c| c["did"].as_str().unwrap().to_string()).collect();
    for c in m["contacts"].as_array().unwrap() {
        node.synth_peer_for_test(c["phrase"].as_str().unwrap().to_string()).unwrap();
    }
    let key = unhex(m["unlock_key"].as_str().unwrap());
    let rss1 = rss_mb();
    let (r, unlock_ms) = timed(|| node.unlock_with_key(key));
    r.unwrap();
    let rss2 = rss_mb();
    let (chats, list_ms) = timed(|| node.chats().unwrap());
    let rss3 = rss_mb();
    out.insert("node_new_ms".into(), json!(round(new_ms)));
    out.insert("unlock_with_key_ms".into(), json!(round(unlock_ms)));
    out.insert("chat_list_first_ms".into(), json!(round(list_ms)));
    out.insert("unlock_to_chat_list_ready_ms".into(), json!(round(unlock_ms + list_ms)));
    out.insert("chat_list_len".into(), json!(chats.len()));
    out.insert("rss_mb".into(), json!({"process_start": round(rss0), "after_node_new": round(rss1), "after_unlock": round(rss2), "after_chat_list": round(rss3)}));

    // Group store open (lazy: the first group call).
    let (gs, groups_ms) = timed(|| node.groups().unwrap());
    let rss4 = rss_mb();
    out.insert("group_store_open_plus_groups_ms".into(), json!(round(groups_ms)));
    out.insert("rss_after_group_open_mb".into(), json!(round(rss4)));
    out.insert("group_store_rss_growth_mb".into(), json!(round(rss4 - rss3)));
    out.insert("groups_len".into(), json!(gs.len()));
    let mut st = Stat(vec![]);
    for _ in 0..10 {
        st.0.push(timed(|| node.groups().unwrap()).1);
    }
    out.insert("groups_call_warm".into(), st.json());

    if scenario == "bg" {
        std::thread::sleep(Duration::from_secs(40));
        out.insert("rss_after_bg_settle_mb".into(), json!(round(rss_mb())));
    }
    node.search_background_reconcile_for_test(false);
    // The dataset's "today" is its last day: that is what the hot window is counted from.
    let end_day = {
        let ms = m["end_ms"].as_i64().unwrap();
        let z = ms.div_euclid(86_400_000) + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let mo = if mp < 10 { mp + 3 } else { mp - 9 };
        format!("{:04}-{mo:02}-{d:02}", yoe + era * 400 + i64::from(mo <= 2))
    };
    node.search_set_today_for_test(Some(end_day)).unwrap();
    let rss5 = rss_mb();
    let ((dms, grps), _) = timed(|| node.search_reconcile_now_for_test().unwrap());
    out.insert("search_reconcile".into(), json!({"dm_ms": round(ms(dms)), "groups_ms": round(ms(grps)), "rss_before_mb": round(rss5), "rss_after_mb": round(rss_mb())}));
    out.insert("disk_after_reconcile".into(), sizes(&root));
    if let Some(f) = arg(args, "--save-search") {
        std::fs::copy(account_dir(&root).join("search.redb"), &f).unwrap();
    }

    let cs = Corpus2::pick(&node, &m);
    if scenario == "apply" {
        let g = m["groups"].as_array().unwrap().last().unwrap();
        let gid = g["gid"].as_str().unwrap().to_string();
        let mem = &g["members"][1];
        node.synth_peer_for_test(mem["phrase"].as_str().unwrap().to_string()).unwrap();
        let mut r = node.synth_remote_open_for_test(gid, mem["did"].as_str().unwrap().to_string()).unwrap();
        let mut rng = Rng(77);
        for i in 0..12 {
            let text = format!("{} r{i}", message(&mut rng, &corpus()));
            let (a, b) = node.synth_remote_post_for_test(&mut r, text, m["end_ms"].as_i64().unwrap() + 1000 + i).unwrap();
            eprintln!("apply {i}: core {:.0} ms, after {:.0} ms", ms(a), ms(b));
        }
        return;
    }
    if scenario == "intact" || scenario == "bg" {
        measure(&node, &m, &did, &contacts, &cs, &mut out);
    } else {
        let (r, t) = timed(|| node.search("meeting".into(), 20, None).unwrap());
        out.insert("check_search_meeting".into(), json!({"ms": round(t), "hits": r.messages.len()}));
    }
    out.insert("peak_rss_mb".into(), json!(round(rss_kb("VmHWM:") as f64 / 1024.0)));
    out.insert("final_rss_mb".into(), json!(round(rss_mb())));
    out.insert("disk_final".into(), sizes(&root));
    println!("{}", serde_json::to_string_pretty(&Value::Object(out)).unwrap());
    let _ = std::fs::remove_dir_all(&work);
}

struct Corpus2;
impl Corpus2 {
    fn pick(_: &Node, _: &Value) -> Corpus2 {
        Corpus2
    }
}

fn measure(node: &Node, m: &Value, did: &str, contacts: &[String], _c: &Corpus2, out: &mut serde_json::Map<String, Value>) {
    let end = m["end_ms"].as_i64().unwrap();
    let corp = corpus();
    let mut rng = Rng(0xabcdef12345);

    // chat_list
    let mut st = Stat(vec![]);
    for _ in 0..30 {
        st.0.push(timed(|| node.chats().unwrap()).1);
    }
    out.insert("chat_list".into(), st.json());

    // chat_day: newest day of 30 different contacts (first touch each), then the same again (cached).
    let mut cold = Stat(vec![]);
    let mut warm = Stat(vec![]);
    let mut nmsg = 0;
    for c in contacts.iter().take(30) {
        let (p, t) = timed(|| node.chat_day(c.clone(), None).unwrap());
        nmsg += p.messages.len();
        cold.0.push(t);
        warm.0.push(timed(|| node.chat_day(c.clone(), None).unwrap()).1);
    }
    out.insert("chat_day_newest_first_touch".into(), cold.json());
    out.insert("chat_day_newest_cached".into(), warm.json());
    out.insert("chat_day_newest_avg_msgs".into(), json!(nmsg / 30));

    // chat_day for a closed day ~200 days back: the nearest day with messages at or before it.
    let d200 = day_of(end - 200 * DAY_MS);
    let mut closed = Stat(vec![]);
    let mut closed_warm = Stat(vec![]);
    let mut cmsg = 0;
    for c in contacts.iter().take(30) {
        let (p, t) = timed(|| node.chat_day(c.clone(), Some(d200.clone())).unwrap());
        // `older_day` when that exact day is empty for this contact (the walk is one call, as the app does it).
        if p.messages.is_empty() {
            if let Some(od) = p.older_day {
                let (p2, t2) = timed(|| node.chat_day(c.clone(), Some(od.clone())).unwrap());
                cmsg += p2.messages.len();
                closed.0.push(t2);
                closed_warm.0.push(timed(|| node.chat_day(c.clone(), Some(od.clone())).unwrap()).1);
            }
        } else {
            cmsg += p.messages.len();
            closed.0.push(t);
            closed_warm.0.push(timed(|| node.chat_day(c.clone(), Some(d200.clone())).unwrap()).1);
        }
    }
    out.insert("chat_day_closed_200d_first_touch".into(), closed.json());
    out.insert("chat_day_closed_200d_cached".into(), closed_warm.json());
    out.insert("chat_day_closed_avg_msgs".into(), json!(cmsg / closed.0.len().max(1)));
    // The heaviest contact (index 0 gets ~19% of all messages): closed days, each touched once, walking back by `older_day`.
    let mut heavy_cold = Stat(vec![]);
    let mut cold_n = 0;
    let mut day = day_of(end - 120 * DAY_MS);
    for _ in 0..25 {
        let (p, t) = timed(|| node.chat_day(contacts[0].clone(), Some(day.clone())).unwrap());
        if !p.messages.is_empty() {
            heavy_cold.0.push(t);
            cold_n += p.messages.len();
        }
        match p.older_day {
            Some(o) => day = o,
            None => break,
        }
    }
    out.insert("chat_day_closed_heaviest_contact_cold".into(), heavy_cold.json());
    out.insert("chat_day_closed_heaviest_cold_avg_msgs".into(), json!(cold_n / heavy_cold.0.len().max(1)));
    // The empty-day call itself (index lookups only, no shard load).
    let mut walk = Stat(vec![]);
    for c in contacts.iter().take(30) {
        walk.0.push(timed(|| node.chat_day(c.clone(), Some(day_of(end - 199 * DAY_MS))).unwrap()).1);
    }
    out.insert("chat_day_closed_second_pass".into(), walk.json());

    // Groups.
    let mut gout = Vec::new();
    for g in m["groups"].as_array().unwrap() {
        let gid = g["gid"].as_str().unwrap().to_string();
        let name = g["name"].as_str().unwrap();
        let mut first = Stat(vec![]);
        for _ in 0..20 {
            first.0.push(timed(|| node.group_messages(gid.clone(), None, 50).unwrap()).1);
        }
        // A deep cursor: the middle of the group.
        let n_items = g["posts"].as_u64().unwrap() as usize;
        let mid = node.group_messages(gid.clone(), None, (n_items / 2).max(1) as u32).unwrap().older;
        let mut deep = Stat(vec![]);
        for _ in 0..20 {
            deep.0.push(timed(|| node.group_messages(gid.clone(), mid.clone(), 50).unwrap()).1);
        }
        // The oldest page.
        let mut oldest = Stat(vec![]);
        let all = node.group_messages(gid.clone(), None, u32::MAX).unwrap();
        let cur = all.messages.get(50).map(|x| format!("{}:{}", x.at, x.id));
        for _ in 0..10 {
            oldest.0.push(timed(|| node.group_messages(gid.clone(), cur.clone(), 50).unwrap()).1);
        }
        // Local writes.
        let mut wr = Stat(vec![]);
        for i in 0..200 {
            let text = format!("{} {i}", message(&mut rng, &corp));
            wr.0.push(timed(|| node.group_post(gid.clone(), text, None).unwrap()).1);
        }
        // Remote applies from three members.
        let mut ap = Stat(vec![]);
        let mut af = Stat(vec![]);
        let mut tot = Stat(vec![]);
        let mems = g["members"].as_array().unwrap();
        let mut open_ms = 0.0;
        for k in 0..3 {
            let mem = &mems[k + 1 % mems.len()];
            node.synth_peer_for_test(mem["phrase"].as_str().unwrap().to_string()).unwrap();
            let mut r;
            let (rr, t) = timed(|| node.synth_remote_open_for_test(gid.clone(), mem["did"].as_str().unwrap().to_string()).unwrap());
            r = rr;
            open_ms += t;
            for i in 0..12 {
                let text = format!("{} r{i}", message(&mut rng, &corp));
                let (a, b) = node.synth_remote_post_for_test(&mut r, text, end + 1000 + i).unwrap();
                ap.0.push(ms(a));
                af.0.push(ms(b));
                tot.0.push(ms(a + b));
            }
        }
        gout.push(json!({"name": name, "posts": n_items, "first_page_50": first.json(), "deep_page_50_middle": deep.json(), "oldest_page_50": oldest.json(), "local_write": wr.json(), "remote_apply_core": ap.json(), "remote_after_hook": af.json(), "remote_total": tot.json(), "remote_open_untimed_avg_ms": round(open_ms / 3.0)}));
    }
    out.insert("groups".into(), Value::Array(gout));
    out.insert("rss_after_group_ops_mb".into(), json!(round(rss_mb())));

    // DM send: 200 sends spread over the contacts.
    let mut first = Stat(vec![]);
    let mut rest = Stat(vec![]);
    let mut seen = std::collections::HashSet::new();
    for i in 0..600 {
        let c = &contacts[i % 40];
        let text = format!("{} s{i}", message(&mut rng, &corp));
        let (_, t) = timed(|| node.send_text(c.clone(), text, None).unwrap());
        if seen.insert(c.clone()) { first.0.push(t) } else { rest.0.push(t) }
    }
    out.insert("dm_send_first_in_today_shard".into(), first.json());
    out.insert("dm_send_subsequent".into(), rest.json());

    // Search.
    let mut sres = serde_json::Map::new();
    for (label, q) in [("common_prefix_th", "th"), ("common_word_the", "the"), ("common_word_meeting", "meeting"), ("two_words_meeting_tomorrow", "meeting tomorrow"), ("rare_word", "zyxqrare"), ("rare_prefix_zyx", "zyx"), ("midfreq_photo_flight", "photo flight")] {
        let mut st = Stat(vec![]);
        let mut hits = 0;
        let mut next = false;
        for _ in 0..10 {
            let (r, t) = timed(|| node.search(q.into(), 20, None).unwrap());
            hits = r.messages.len();
            next = r.next.is_some();
            st.0.push(t);
        }
        // A deep page: follow the cursor 5 pages.
        let mut cur = node.search(q.into(), 20, None).unwrap().next;
        let mut deep = Stat(vec![]);
        for _ in 0..5 {
            if cur.is_none() {
                break;
            }
            let (r, t) = timed(|| node.search(q.into(), 20, cur.clone()).unwrap());
            cur = r.next;
            deep.0.push(t);
        }
        let mut j = st.json();
        j["page_hits"] = json!(hits);
        j["has_next"] = json!(next);
        j["pages_2_to_6"] = deep.json();
        sres.insert(label.into(), j);
    }
    out.insert("search_limit20".into(), Value::Object(sres));
    let _ = did;
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("gen") => generate(&args),
        Some("run") => run(&args),
        Some("opentest") => {
            for i in 0..4 {
                let t = Instant::now();
                let r = storage::Store::open(&args[1]);
                eprintln!("open #{i}: {:?} in {:?}", r.as_ref().err(), t.elapsed());
                drop(r);
            }
        }
        Some("dirtywrite") => {
            let node = Node::new(args[1].clone(), Arc::new(Quiet)).unwrap();
            node.unlock_with_key(unhex(&args[2])).unwrap();
            for i in 0..5 {
                node.send_text(args[3].clone(), format!("dirty write {i}"), None).unwrap();
            }
            node.group_post(args[4].clone(), "dirty group write".into(), None).unwrap();
            // Gone without closing anything.
            std::process::exit(0);
        }
        _ => eprintln!("usage: year-bench gen|run ..."),
    }
}
