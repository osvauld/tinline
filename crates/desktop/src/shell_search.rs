//! Real local search. Query generations keep slow results out of newer queries/accounts.
use super::*;
use p2pcore::{SearchResults, NameHit, SearchNameKind};

#[derive(Default)]
pub(super) struct State {
    pub active: bool,
    pub generation: u64,
    pub result: Option<SearchResults>,
    pub busy: bool,
    pub error: Option<String>,
    account: Option<String>,
}
impl State {
    pub fn clear(&mut self) {
        self.generation += 1;
        self.result = None; self.busy = false; self.error = None;
    }
    pub fn bind(&mut self, account: Option<&str>) -> bool {
        if self.account.as_deref() == account { return false; }
        self.account = account.map(str::to_owned);
        self.clear(); self.active = false; true
    }
    pub fn accept(&mut self, generation: u64, append: bool, result: Result<SearchResults, String>) {
        if generation != self.generation { return; }
        self.busy = false;
        match result {
            Ok(mut page) => {
                if append && let Some(old) = self.result.as_mut() {
                    page.messages.retain(|m| !old.messages.iter().any(|o| o.kind == m.kind && o.conv == m.conv && o.id == m.id));
                    old.messages.extend(page.messages); old.next = page.next;
                } else { self.result = Some(page); }
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }
}
#[derive(Debug, Clone)]
pub(super) enum Action {
    Focus, Escape, More, Tab(chat::SideTab),
    Ready(u64, bool, Result<SearchResults, String>),
    OpenName(NameHit),
}
impl App {
    pub(super) fn shell_search_query(&mut self, value: String) -> Task<Msg> {
        self.search = value;
        self.shell_search.active = true;
        self.shell_search.clear();
        if self.search.trim().is_empty() { return Task::none(); }
        self.shell_search.busy = true;
        let (node, query, generation) = (self.node.clone(), self.search.trim().to_string(), self.shell_search.generation);
        Task::perform(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            tokio::task::spawn_blocking(move || node.search(query, 20, None).map_err(friendly)).await.unwrap_or_else(|e| Err(e.to_string()))
        }, move |result| Msg::ShellSearch(Action::Ready(generation, false, result)))
    }
    pub(super) fn shell_search_update(&mut self, action: Action) -> Task<Msg> {
        match action {
            Action::Tab(tab) => {
                self.search.clear(); self.shell_search.clear(); self.shell_search.active = false;
                let home = self.update(Msg::Home);
                return Task::batch([home, self.update_chat(Cm::Tab(tab))]);
            }
            Action::Focus => { self.shell_search.active = true; return operation::focus(SEARCH_ID); }
            Action::Escape if !self.search.is_empty() => return self.shell_search_query(String::new()),
            Action::Escape => { self.shell_search.active = false; return operation::focus("shell-nav"); }
            Action::Ready(generation, append, mut result) => {
                if self.search.trim().chars().count() < 2 && let Ok(page) = &mut result { page.messages.clear(); page.next = None; }
                self.shell_search.accept(generation, append, result);
            }
            Action::More => {
                if self.shell_search.busy { return Task::none(); }
                let Some(cursor) = self.shell_search.result.as_ref().and_then(|r| r.next.clone()) else { return Task::none() };
                self.shell_search.busy = true;
                let (node, query, generation) = (self.node.clone(), self.search.clone(), self.shell_search.generation);
                return blocking(move || node.search(query, 20, Some(cursor)).map_err(friendly), move |r| Msg::ShellSearch(Action::Ready(generation, true, r)));
            }
            Action::OpenName(hit) => match hit.kind {
                SearchNameKind::Contact | SearchNameKind::Me => return self.update(Msg::Select(hit.id)),
                SearchNameKind::Group => return self.update_live(Lm::Open(hit.id)),
            },
        }
        Task::none()
    }
}

/// Unicode scalar offsets from core, not bytes or UTF-16. Keep 18 leading chars before the match.
pub(super) fn marked_parts(text: &str, ranges: &[p2pcore::HitRange]) -> Vec<(String, bool)> {
    let chars: Vec<char> = text.chars().collect();
    let start = ranges.first().map_or(0, |r| (r.start as usize).saturating_sub(18)).min(chars.len());
    let mut parts: Vec<(String, bool)> = Vec::new();
    if start > 0 { parts.push(("…".into(), false)); }
    for (i, c) in chars.iter().enumerate().skip(start) {
        let found = ranges.iter().any(|r| (r.start as usize) <= i && i < r.end as usize);
        if let Some((s, previous)) = parts.last_mut().filter(|(_, p)| *p == found) { let _ = previous; s.push(*c); }
        else { parts.push((c.to_string(), found)); }
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    fn page() -> SearchResults { SearchResults { names: vec![], messages: vec![], next: None } }
    #[test]
    fn actual_node_search_returns_real_saved_messages_newest_first() {
        struct Quiet;
        impl p2pcore::NodeEvents for Quiet {
            fn on_status(&self, _: NodeStatus) {}
            fn on_contacts_changed(&self) {}
            fn on_incoming_call(&self, _: CallInfo) {}
            fn on_call_state(&self, _: String, _: CallState) {}
            fn on_log(&self, _: String) {}
        }
        let suffix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("screens-shell-search-{}-{suffix}", std::process::id()));
        let node = Node::new(dir.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
        node.create_identity("Search check".into(), String::new()).unwrap();
        node.commit_identity().unwrap();
        let me = keystore::current_did(&node).unwrap();
        let first = node.send_text(me.clone(), "first sol61needle message".into(), None).unwrap();
        std::thread::sleep(Duration::from_millis(2));
        let second = node.send_text(me, "newest sol61needle message".into(), None).unwrap();
        let result = node.search("sol61needle".into(), 20, None).unwrap();
        assert_eq!(result.messages.len(), 2);
        assert_eq!(result.messages[0].id, second.id); assert_eq!(result.messages[1].id, first.id);
        assert!(!result.messages[0].highlights.is_empty());
        drop(node); let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn stale_results_are_rejected_after_query_or_account_change() {
        let mut s = State::default(); s.clear(); let old = s.generation;
        s.clear(); s.accept(old, false, Ok(page())); assert!(s.result.is_none());
        let current = s.generation; s.accept(current, false, Ok(page())); assert!(s.result.is_some());
        assert!(s.bind(Some("different account"))); assert!(s.result.is_none());
        s.accept(current, false, Ok(page())); assert!(s.result.is_none());
    }
    #[test]
    fn highlighting_uses_scalar_offsets_after_emoji_and_indic_marks() {
        let p = marked_parts("😀 ऊ lunch", &[p2pcore::HitRange { start: 4, end: 9 }]);
        assert_eq!(p, vec![("😀 ऊ ".into(), false), ("lunch".into(), true)]);
    }
    #[test]
    fn errors_are_not_silently_reported_as_no_matches() {
        let mut s = State::default(); s.busy = true;
        s.accept(0, false, Err("Account is locked".into()));
        assert!(!s.busy); assert_eq!(s.error.as_deref(), Some("Account is locked"));
    }
}
