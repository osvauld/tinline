use super::*;
#[test]
fn draft_and_reply_survive_switching_chats() {
    let mut m = Memory::default(); m.suspend("a", "unsent".into(), Some("reply".into()), false);
    m.suspend("b", "other".into(), None, false);
    assert_eq!(m.restore("a"), Draft { text: "unsent".into(), reply: Some("reply".into()) });
}
#[test]
fn leaving_edit_restores_original_draft_not_edit_text() {
    let mut m = Memory::default(); m.start_edit("my draft".into()); m.start_edit("another edit".into());
    m.suspend("a", "modified message".into(), None, true);
    assert_eq!(m.restore("a").text, "my draft"); assert!(m.edit_stash.is_none());
}
#[test]
fn leaving_while_edit_is_submitted_preserves_borrowed_draft() {
    let mut m = Memory::default(); m.start_edit("original draft".into());
    m.suspend("a", String::new(), None, false);
    assert_eq!(m.restore("a").text, "original draft");
}
#[test]
fn new_typing_during_io_is_not_replaced_with_edit_stash() {
    let mut m = Memory::default(); m.start_edit("old draft".into());
    m.suspend("a", "new typing".into(), None, false);
    assert_eq!(m.restore("a").text, "new typing"); assert!(m.edit_stash.is_none());
}
#[test]
fn sending_does_not_erase_a_different_stored_draft() {
    let mut m = Memory::default(); m.suspend("a", "new draft".into(), None, false);
    m.clear_sent("a", "old text"); assert_eq!(m.restore("a").text, "new draft");
    m.clear_sent("a", "new draft"); assert_eq!(m.restore("a"), Draft::default());
}
#[test]
fn cancelling_edit_returns_stash_once() {
    let mut m = Memory::default(); m.start_edit("draft".into());
    assert_eq!(m.finish_edit(), "draft"); assert_eq!(m.finish_edit(), "");
}
#[test]
fn submission_is_rejected_after_account_epoch_changes() {
    let s = Submission { epoch: 7, text: "private draft".into(), reply: None, editing: None };
    assert!(s.is_current(7));
    assert!(!s.is_current(8));
    assert!(!s.is_current(0));
}
#[test]
fn pluck_requires_successful_new_send_not_edit_or_failure() {
    let mut s = Submission { epoch: 0, text: "hi".into(), reply: None, editing: None };
    assert!(!s.pluck(false)); assert!(s.pluck(true));
    s.editing = Some("old".into()); assert!(!s.pluck(true));
}
