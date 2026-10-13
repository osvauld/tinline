//! Per-account, in-session draft/reply memory. Editing borrows the field; it never replaces its draft.
use std::collections::HashMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Draft { pub text: String, pub reply: Option<String> }
#[derive(Default)]
pub(super) struct Memory {
    drafts: HashMap<String, Draft>,
    pub edit_stash: Option<String>,
}
impl Memory {
    pub fn start_edit(&mut self, draft: String) { if self.edit_stash.is_none() { self.edit_stash = Some(draft); } }
    pub fn finish_edit(&mut self) -> String { self.edit_stash.take().unwrap_or_default() }
    pub fn suspend(&mut self, peer: &str, text: String, reply: Option<String>, editing: bool) {
        let draft = if editing || (self.edit_stash.is_some() && text.is_empty()) {
            Draft { text: self.finish_edit(), reply: None }
        } else { self.edit_stash = None; Draft { text, reply } };
        self.drafts.insert(peer.to_string(), draft);
    }
    pub fn restore(&self, peer: &str) -> Draft { self.drafts.get(peer).cloned().unwrap_or_default() }
    pub fn clear_sent(&mut self, peer: &str, sent: &str) {
        if self.drafts.get(peer).is_some_and(|d| d.text == sent || d.text.is_empty()) { self.drafts.remove(peer); }
    }
}
#[derive(Debug, Clone)]
pub(crate) struct Submission { pub epoch: u64, pub text: String, pub reply: Option<String>, pub editing: Option<String> }
impl Submission {
    pub fn is_current(&self, epoch: u64) -> bool { self.epoch == epoch }
    pub fn pluck(&self, success: bool) -> bool { success && self.editing.is_none() }
}

#[cfg(test)]
mod tests {
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
}
