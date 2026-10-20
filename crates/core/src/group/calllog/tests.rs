use super::*;
use GroupCallEventKind as K;

const G: &str = "gid1";
const C: &str = "call1";
fn dids(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("did:key:synthetic{i}")).collect()
}
fn ev(l: &mut CallLog, k: K, p: &[String], t: u64) -> Option<Finished> {
    l.apply(k, G, C, p, "did:key:synthetic0", t, None)
}

#[test]
fn joined_then_left_records_duration_and_peak() {
    let mut l = CallLog::default();
    assert!(ev(&mut l, K::Announced, &[], 1_000).is_none());
    ev(&mut l, K::Joined, &[], 2_000);
    ev(&mut l, K::RosterChanged, &dids(2), 3_000);
    ev(&mut l, K::RosterChanged, &dids(4), 4_000);
    ev(&mut l, K::RosterChanged, &dids(3), 5_000);
    let f = ev(&mut l, K::Left, &[], 62_000).unwrap();
    let r = merge(None, &f);
    assert_eq!((r.duration_secs, r.people, r.missed, r.started_at), (60, 4, false, 1));
    assert_eq!(r.started_by, "did:key:synthetic0");
    // The host's End arriving after our Left makes no second record.
    assert!(ev(&mut l, K::Ended, &[], 70_000).is_none());
}

#[test]
fn never_joined_then_ended_is_missed_from_announce() {
    let mut l = CallLog::default();
    ev(&mut l, K::Announced, &[], 10_000);
    let f = ev(&mut l, K::Ended, &[], 310_000).unwrap();
    let r = merge(None, &f);
    assert_eq!((r.duration_secs, r.people, r.missed), (300, 0, true));
    assert!(ev(&mut l, K::Ended, &[], 311_000).is_none());
}

#[test]
fn ended_while_joined_records_once_and_unseen_calls_record_nothing() {
    let mut l = CallLog::default();
    ev(&mut l, K::Announced, &[], 0);
    ev(&mut l, K::Joined, &[], 1_000);
    let f = ev(&mut l, K::Ended, &[], 121_000).unwrap();
    assert_eq!((merge(None, &f).duration_secs, merge(None, &f).missed), (120, false));
    assert!(ev(&mut l, K::Left, &[], 121_500).is_none());
    let mut l = CallLog::default();
    assert!(ev(&mut l, K::Ended, &[], 5).is_none());
    assert!(ev(&mut l, K::Left, &[], 6).is_none());
}

#[test]
fn host_move_is_one_session() {
    let mut l = CallLog::default();
    ev(&mut l, K::Announced, &[], 0);
    ev(&mut l, K::Joined, &[], 1_000);
    ev(&mut l, K::Reconnecting, &[], 20_000);
    ev(&mut l, K::Joined, &[], 22_000); // on the backup host (gen + 1)
    ev(&mut l, K::Announced, &[], 22_000); // takeover re-announce
    let f = ev(&mut l, K::Left, &[], 61_000).unwrap();
    assert_eq!(merge(None, &f).duration_secs, 60);
    assert!(ev(&mut l, K::Ended, &[], 62_000).is_none());
}

#[test]
fn rejoin_adds_to_the_same_record_and_duplicate_dids_count_once() {
    let mut l = CallLog::default();
    ev(&mut l, K::Joined, &[], 0);
    let mut p = dids(2);
    p.push(p[0].clone());
    ev(&mut l, K::RosterChanged, &p, 1);
    let r = merge(None, &ev(&mut l, K::Left, &[], 30_000).unwrap());
    assert_eq!((r.duration_secs, r.people), (30, 2));
    ev(&mut l, K::Joined, &[], 40_000);
    ev(&mut l, K::RosterChanged, &dids(3), 41_000);
    let r = merge(Some(r), &ev(&mut l, K::Left, &[], 70_000).unwrap());
    assert_eq!((r.duration_secs, r.people, r.missed), (60, 3, false));
}

#[test]
fn labels_follow_the_design() {
    let mk = |secs, people, missed| GroupCallRecord { call_id: "c".into(), group_id: "g".into(), started_by: String::new(), started_at: 0, duration_secs: secs, people, missed };
    assert_eq!(group_call_label(mk(19 * 60, 4, false)), "Group call \u{b7} 19 min \u{b7} 4 people");
    assert_eq!(group_call_label(mk(45, 1, false)), "Group call \u{b7} 45 s");
    assert_eq!(group_call_label(mk(300, 0, true)), "Missed group call \u{b7} 5 min");
}

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
