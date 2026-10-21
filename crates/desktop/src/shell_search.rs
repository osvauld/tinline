//! Real local search. Query generations keep slow results out of newer queries/accounts.
use super::*;
use p2pcore::{SearchResults, NameHit, SearchNameKind};

pub(super) const NEW_CHAT_ID: &str = "new-chat-find";

#[derive(Default)]
pub(super) struct State {
    pub active: bool,
    pub generation: u64,
    pub result: Option<SearchResults>,
    pub busy: bool,
    pub error: Option<String>,
    account: Option<String>,
    /// ChatsList filter chips (All / Unread).
    pub unread_only: bool,
    /// NewChat.svelte: open with its "Find a contact" text.
    pub new_chat: Option<String>,
    /// ContactsList "Find a contact".
    pub contacts_q: String,
}
impl State {
    pub fn clear(&mut self) {
        self.generation += 1;
        self.result = None; self.busy = false; self.error = None;
    }
    pub fn bind(&mut self, account: Option<&str>) -> bool {
        if self.account.as_deref() == account { return false; }
        self.account = account.map(str::to_owned);
        self.clear(); self.active = false; self.new_chat = None; self.unread_only = false; true
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
    /// Chats filter chip.
    Unread(bool),
    /// Open / close the New chat picker; its find text; a contact chosen; its shortcuts.
    ContactsFind(String), OpenPerson(String),
    NewChat(bool), NewChatFind(String), NewChatPick(String), NewChatAdd, NewChatGroup,
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
            Action::Unread(on) => self.shell_search.unread_only = on,
            Action::ContactsFind(q) => self.shell_search.contacts_q = q,
            // Desktop `nav.open({ name: 'contact' })`: the person's profile replaces the detail pane.
            Action::OpenPerson(did) => {
                self.profiles.history.clear();
                self.profiles.page = None;
                return self.profile_update(profile_ui::Action::Open(profile_ui::Page::Person(did)));
            }
            Action::NewChat(open) => {
                self.shell_search.new_chat = open.then(String::new);
                if open { return operation::focus(NEW_CHAT_ID); }
            }
            Action::NewChatFind(q) => if let Some(f) = self.shell_search.new_chat.as_mut() { *f = q; },
            Action::NewChatPick(did) => { self.shell_search.new_chat = None; return self.update(Msg::Select(did)); }
            Action::NewChatAdd => { self.shell_search.new_chat = None; return self.update(Msg::OpenAdd); }
            Action::NewChatGroup => {
                self.shell_search.new_chat = None;
                let tab = self.shell_search_update(Action::Tab(chat::SideTab::Groups));
                return Task::batch([tab, self.update_live(Lm::NewGroup)]);
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
#[path = "shell_search_tests.rs"]
mod tests;
