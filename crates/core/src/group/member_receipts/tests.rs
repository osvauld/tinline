use super::*;

#[test]
fn frontier_inclusion_is_per_writer_and_exclusive_end() {
    assert!(covers(&[(1, 3), (5, 2)], &[(1, 3)]));
    assert!(covers(&[(1, 3), (5, 2)], &[]));
    assert!(!covers(&[(1, 2), (5, 2)], &[(1, 3)]));
    assert!(!covers(&[(5, 2)], &[(1, 1)]));
    assert!(covers(&[(1, 9), (5, 2), (7, 1)], &[(1, 4), (7, 1)]));
}

#[test]
fn nobody_to_confirm_keeps_the_mark_at_one_tick() {
    let post = PostRef {
        id: "p".into(),
        author: "did:me".into(),
        seen: vec![],
        vis: Vis::Visible,
    };
    let s = aggregate(&GroupState::default(), "did:me", &post, &[1; 32], &[]);
    assert_eq!(
        (s.tick, s.eligible, s.delivered, s.read),
        (ReceiptTick::Waiting, 0, 0, 0)
    );
    let theirs = aggregate(&GroupState::default(), "did:other", &post, &[1; 32], &[]);
    assert_eq!(theirs.tick, ReceiptTick::None);
}

#[test]
fn refusal_messages_mark_what_only_waits() {
    assert!(Refusal::Pending("x").message().starts_with(PENDING_PREFIX));
    assert!(!Refusal::Rejected("x").message().starts_with(PENDING_PREFIX));
}
