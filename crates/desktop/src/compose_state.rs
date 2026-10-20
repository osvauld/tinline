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
#[path = "compose_state_tests.rs"]
mod tests;
