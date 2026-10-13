//! DesktopShell.svelte: 232px navigation | 380px list | detail. Local search replaces the list.
use super::*;
use iced::widget::{column, rich_text, span};
use super::super::shell_search::Action;
use super::super::chat::SideTab;

impl App {
    pub(super) fn sidebar(&self, t: Tok) -> El<'_> {
        row![self.shell_navigation(t), self.list_pane(t)].height(Fill).into()
    }
    fn shell_navigation(&self, t: Tok) -> El<'_> {
        let brand = container(row![ui::logo_live(28.0, t, self.mark_look()), semi("Tinline", 19.0, t.ink)].spacing(8).align_y(Alignment::Center))
            .padding(Padding { top: 4.0, right: 8.0, bottom: 16.0, left: 8.0 });
        let mut nav = column![brand].spacing(2);
        for (label, tab, icon) in [("Chats", SideTab::Chats, Icon::MessageSquare), ("Groups", SideTab::Groups, Icon::Users),
            ("Calls", SideTab::Calls, Icon::Phone), ("Contacts", SideTab::Contacts, Icon::UserPlus)] {
            let on = self.chat.tab == tab;
            let color = if on { t.on_primary_c } else { t.ink2 };
            let badge = if tab == SideTab::Chats { self.chat.unread_total() + self.live.pending_in().count() as u32 } else { 0 };
            let caption = row![ui::icon(icon, 20.0, color), semi(label, 15.0, color).width(Fill),
                container(semi(if badge > 99 { "99+".into() } else if badge > 0 { badge.to_string() } else { String::new() }, 11.0, color)).width(32).center_x(32)]
                .spacing(12).align_y(Alignment::Center);
            nav = nav.push(button(container(caption).height(44).center_y(44)).width(Fill).height(44).padding([0, 12])
                .style(ui::button_style(t, if on { Kind::Tonal } else { Kind::Ghost }, 14.0))
                .on_press(Msg::ShellSearch(Action::Tab(tab))));
        }
        // Keep the existing account-switch menu; Settings remains a separate, genuine entry.
        let me = button(container(row![ui::icon(Icon::Sliders, 20.0, t.ink2), semi("Settings", 15.0, t.ink)].spacing(12))
            .height(44).center_y(44)).padding([0, 12]).width(Fill).height(44)
            .style(ui::button_style(t, Kind::Ghost, 14.0)).on_press(Msg::OpenSettings);
        container(column![nav, Space::new().height(Fill), self.avail_footer(t), self.account_chip(t), me].spacing(8))
            .padding([16, 12]).width(232).height(Fill).style(ui::plain(t.bg, 0.0)).into()
    }
    pub(super) fn desktop_search_field(&self, t: Tok) -> El<'_> {
        container(text_input("Search (Ctrl+K)", &self.search).id(SEARCH_ID).on_input(Msg::SearchIn)
            .padding(12).size(15).style(ui::input_style(t)).width(Fill))
            .padding(Padding { top: 12.0, right: 12.0, bottom: 4.0, left: 12.0 }).width(Fill).into()
    }
    pub(super) fn shell_search_results(&self, t: Tok) -> El<'_> {
        let heading = |caption| container(label(t, caption)).height(36).padding([0, 16]).center_y(36);
        let result = self.shell_search.result.as_ref();
        let mut tiles = row![].spacing(2);
        if let Some(result) = result {
            for hit in &result.names {
                tiles = tiles.push(button(column![avatar(t, &hit.name, &hit.id, 48.0),
                    container(tx(hit.name.clone(), 12.0, t.ink)).height(30).width(70).clip(true)]
                    .spacing(6).align_x(Alignment::Center)).width(78).height(92).padding([6, 4])
                    .style(ui::button_style(t, Kind::Ghost, 14.0)).on_press(Msg::ShellSearch(Action::OpenName(hit.clone()))));
            }
        }
        let names: El = if result.is_some_and(|r| !r.names.is_empty()) {
            scroll(t, tiles).direction(iced::widget::scrollable::Direction::Horizontal(Default::default())).height(100).into()
        } else { container(tx(if self.search.trim().is_empty() { "Search messages, people and groups." } else { "No people or groups match" }, 13.0, t.ink2)).height(100).center_x(Fill).center_y(100).into() };
        let mut messages = column![].spacing(0);
        if let Some(result) = result {
            for hit in &result.messages {
                let parts = super::super::shell_search::marked_parts(&hit.snippet, &hit.highlights);
                let spans = parts.into_iter().map(|(s, found)| span::<(), iced::Font>(s).font(if found { ui::SANS_BOLD } else { ui::SANS }).color(if found { t.ink } else { t.ink2 })).collect::<Vec<_>>();
                let title = row![container(semi(hit.conv_name.clone(), 15.0, t.ink).wrapping(iced::widget::text::Wrapping::None)).width(Fill).clip(true),
                    container(tx(hit.day.clone(), 11.0, t.ink2)).width(72)].spacing(8);
                // Message results are deliberately read-only until the chat/group jump-and-highlight hook lands.
                // Do not wire a misleading latest-message navigation as if it were an exact hit jump.
                messages = messages.push(container(row![avatar(t, &hit.conv_name, &hit.conv, 44.0),
                    column![title, container(rich_text(spans).size(13).wrapping(iced::widget::text::Wrapping::None)).height(18).width(Fill).clip(true)].spacing(2).width(Fill)]
                    .spacing(12).align_y(Alignment::Center)).height(64).padding([10, 8]).width(Fill));
            }
        }
        let hint = if let Some(error) = &self.shell_search.error { error.as_str() }
            else if self.shell_search.busy { "Searching…" }
            else if self.search.trim().chars().count() < 2 && !self.search.is_empty() { "Type 2 or more characters to search messages." }
            else if result.is_some_and(|r| r.messages.is_empty()) { "No messages match" } else { "" };
        let more = button(tx("More messages", 13.0, t.primary)).height(44).padding([0, 16])
            .style(ui::button_style(t, Kind::Ghost, 14.0)).on_press_maybe(
                (!self.shell_search.busy && result.is_some_and(|r| r.next.is_some())).then_some(Msg::ShellSearch(Action::More)));
        column![heading("People & groups"), names, heading("Messages"),
            container(tx(hint, 13.0, t.ink2)).height(44).width(Fill).center_x(Fill).center_y(44),
            scroll(t, messages).height(Fill), more].width(Fill).height(Fill).into()
    }
    pub(super) fn shell_empty_detail(&self, t: Tok) -> El<'_> {
        container(column![ui::logo(56.0, t), semi("Choose a chat", 15.0, t.ink), tx("Messages stay on your devices.", 13.0, t.ink2)]
            .spacing(6).align_x(Alignment::Center)).width(Fill).height(Fill).center_x(Fill).center_y(Fill).into()
    }
}
