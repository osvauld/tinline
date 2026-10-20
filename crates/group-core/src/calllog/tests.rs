use super::*;

#[test]
fn records_round_trip_through_the_vault_newest_first() {
    let s = GroupStore::in_memory("alpha beta gamma");
    let mk = |id: &str, g: &str, at: u64, missed: bool| Finished { call_id: id.into(), gid: g.into(), starter: "did:key:synthetic0".into(), started_ms: at * 1000, in_call_ms: 90_000, peak: if missed { 0 } else { 3 }, missed };
    s.clog_record(&mk("a", "g1", 100, false)).unwrap();
    s.clog_record(&mk("b", "g1", 300, true)).unwrap();
    s.clog_record(&mk("c", "g2", 200, false)).unwrap();
    // Same call again: merged, not duplicated.
    let again = s.clog_record(&mk("a", "g1", 100, false)).unwrap();
    assert_eq!(again.duration_secs, 180);
    let all = s.clog_list(None).unwrap();
    assert_eq!(all.iter().map(|r| r.call_id.as_str()).collect::<Vec<_>>(), ["b", "c", "a"]);
    assert_eq!(s.clog_list(Some("g1")).unwrap().len(), 2);
    assert!(s.clog_list(Some("g1")).unwrap()[0].missed);
    assert!(s.clog_list(Some("nope")).unwrap().is_empty());
}
