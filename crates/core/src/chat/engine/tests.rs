use super::*;

#[test]
fn a_contact_session_cannot_name_the_you_docs() {
    let (me, a) = ("did:key:me", "did:key:a");
    let with_a = pair_id(me, a);
    let you = pair_id(me, me);
    assert_ne!(with_a, you);
    assert_eq!(doc_day_of(&with_a, &doc_name(&with_a, "2026-10-09")).unwrap(), "2026-10-09");
    assert!(doc_day_of(&with_a, &doc_name(&you, "2026-10-09")).is_err());
    assert!(doc_day_of(&with_a, "you/2026-10-09").is_err());
    assert!(doc_day_of(&with_a, &doc_name(&with_a, "2026-13-09")).is_err());
}

fn rec(id: &str, author: &str, at: i64) -> MsgRec {
    MsgRec { id: id.into(), author: author.into(), at, text: "x".into(), edited_at: None, deleted: false, reply_to: None, file: None, dev: [1; 32], did_sig: vec![7; 64] }
}

/// A read step covers every unread incoming version handed to it, closed (old) days included, and
/// only the newest `MAX_READ_VERSIONS`; mine, deleted and already-read versions never.
#[test]
fn read_step_covers_old_closed_days_and_stays_bounded() {
    use super::receipts::{MAX_READ_VERSIONS, read_versions};
    let (me, them) = ("did:key:me", "did:key:them");
    let pair = pair_id(me, them);
    let old_day = "2026-09-01";
    let old_at = crate::chat::doc::day_start(old_day).unwrap() + 1000;
    let mut old = std::collections::BTreeMap::new();
    old.insert("a".to_string(), rec("a", them, old_at));
    old.insert("mine".to_string(), rec("mine", me, old_at + 1));
    let mut gone = rec("gone", them, old_at + 2);
    gone.deleted = true;
    old.insert("gone".to_string(), gone);
    old.insert("read".to_string(), rec("read", them, old_at - 5000));
    let got = read_versions(&pair, me, old_at - 1, old_at + 10, &[(old_day.to_string(), old.clone())]);
    assert_eq!(got.get(old_day).map(Vec::len), Some(1), "only the unread incoming undeleted one");
    // Bounded: the newest MAX_READ_VERSIONS only.
    let mut many = std::collections::BTreeMap::new();
    for i in 0..(MAX_READ_VERSIONS + 10) {
        let id = format!("m{i}");
        many.insert(id.clone(), rec(&id, them, old_at + i as i64));
    }
    let got = read_versions(&pair, me, 0, i64::MAX, &[(old_day.to_string(), many)]);
    assert_eq!(got.values().map(Vec::len).sum::<usize>(), MAX_READ_VERSIONS);
}
