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
