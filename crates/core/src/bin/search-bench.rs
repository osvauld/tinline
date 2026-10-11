//! `search-bench <messages> [dir]`: local search index benchmark, see docs/search.md.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let n = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(10_000);
    let dir = a.get(2).map(std::path::PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("search-bench"));
    p2pcore::search_bench(n, &dir);
    let _ = std::fs::remove_dir_all(&dir);
}
