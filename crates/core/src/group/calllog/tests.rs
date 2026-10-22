use super::*;

#[test]
fn labels_follow_the_design() {
    let mk = |secs, people, missed| GroupCallRecord { call_id: "c".into(), group_id: "g".into(), started_by: String::new(), started_at: 0, duration_secs: secs, people, missed };
    assert_eq!(group_call_label(mk(19 * 60, 4, false)), "Group call \u{b7} 19 min \u{b7} 4 people");
    assert_eq!(group_call_label(mk(45, 1, false)), "Group call \u{b7} 45 s");
    assert_eq!(group_call_label(mk(300, 0, true)), "Missed group call \u{b7} 5 min");
}
