//! DesktopShell.svelte: 232px navigation | 380px list | detail, no gaps between the columns; each
//! column draws its own 1px hairline end edge. Local search replaces the list while it is active.
use super::*;
use iced::widget::{column, rich_text, span, stack};
use super::super::shell_search::Action;
use super::super::chat::SideTab;
use crate::motion::{dur, Ease};
use std::time::Instant;
use crate::pg_icons::pgi;

/// Sidebar nav rows: 44px with a 2px gap (`nav { gap: 2px }`, `.nav { min-height: 44px }`).
const NAV_STEP: f32 = 46.0;
pub(super) fn tabs() -> [(SideTab, &'static str, &'static str); 4] {
    [(SideTab::Chats, crate::i18n::tr_str("common_chats"), "chat"), (SideTab::Groups, crate::i18n::tr_str("common_groups"), "users"), (SideTab::Calls, crate::i18n::tr_str("common_calls"), "phone"), (SideTab::Contacts, crate::i18n::tr_str("common_contacts"), "contact")]
}

/// tokens.css `.t-badge`: 18px pill, 11px/700 tabular, accent fill (muted: hairline ring, no fill).
pub(super) fn t_badge<'a>(t: Tok, n: u32, muted: bool) -> El<'a> {
    let label = if n > 99 { "99+".to_string() } else { n.to_string() };
    let fg = if muted { t.ink2 } else { t.on_accent };
    let style = move |_: &iced::Theme| iced::widget::container::Style {
        background: (!muted).then_some(iced::Background::Color(t.accent)),
        border: Border { color: if muted { t.line } else { Color::TRANSPARENT }, width: if muted { 1.0 } else { 0.0 }, radius: 9.0.into() },
        ..Default::default()
    };
    container(text(label).size(11).font(if muted { ui::SANS_SEMI } else { ui::SANS_BOLD }).color(fg).line_height(lh(11.0)))
        .padding([0, 5]).height(18).center_y(18).center_x(Length::Shrink).style(style).into()
}

/// `.press` row/button look: transparent, `hover` wash on hover; `fill` when selected.
pub(super) fn wash(t: Tok, fill: Option<Color>, hover: Option<Color>, radius: f32) -> impl Fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style {
    move |_, status| {
        use iced::widget::button::Status as S;
        let bg = match (fill, status) {
            (_, S::Hovered | S::Pressed) if hover.is_some() => hover,
            (f, _) => f,
        };
        iced::widget::button::Style {
            background: bg.map(iced::Background::Color),
            text_color: t.ink,
            border: Border { radius: radius.into(), ..Default::default() },
            ..Default::default()
        }
    }
}

impl App {
    /// Keeps the nav highlight gliding to the current tab (`.hl` transform, --dur-indicator, ease-out).
    pub(in crate::app) fn settle_motion(&mut self) {
        let index = tabs().iter().position(|(tab, ..)| *tab == self.chat.tab).unwrap_or(0) as f32;
        let y = index * NAV_STEP;
        if self.fx.nav.target() != y {
            let now = Instant::now();
            self.fx.now = now;
            self.fx.nav.go(y, dur::INDICATOR, Ease::Out, now);
        }
        let clear = if self.search.is_empty() { 0.0 } else { 1.0 };
        if self.fx.clear.target() != clear {
            let now = Instant::now();
            self.fx.now = now;
            self.fx.clear.go(clear, dur::CROSSFADE, Ease::Out, now);
        }
        let (level, wave) = self.signal_now();
        self.fx.signal(level, wave);
    }

    pub(super) fn shell_navigation(&self, t: Tok) -> El<'_> {
        // .brand: 4 8 16 padding, 8 gap, 19px bold "Tinline" beside the live mark.
        // While a call rings, RingHead takes the brand row's box (32 tall above its 16 px of spacing).
        let brand_row: El = match self.ring_head(t) {
            Some(rh) => rh,
            None => row![ui::logo_live(28.0, t, self.mark_look()), bold("Tinline", 19.0, t.ink).line_height(lh(21.0))].spacing(8).align_y(Alignment::Center).height(32).into(),
        };
        let brand = container(brand_row).padding(Padding { top: 4.0, right: 8.0, bottom: 16.0, left: 8.0 });
        let mut items = column![].spacing(2);
        for (tab, label, icon) in tabs() {
            let on = self.chat.tab == tab;
            let color = if on { t.accent_ink } else { t.ink2 };
            // DesktopShell `unread`: every chat's unread, 1:1 and group (no mute in the core yet).
            // Groups (DesktopShell): invitations waiting for me, so an invite shows on the tab the moment it arrives.
            let badge = match tab {
                SideTab::Chats => self.chat.unread_total() + self.live.list.iter().filter(|g| crate::app::group_live::active(g.my_role)).map(|g| g.unread).sum::<u32>() + self.live.pending_in().count() as u32,
                SideTab::Groups => self.live.pending_invitations().count() as u32,
                // People waiting for my answer: their cards are on Contacts ("Not now" ones aside).
                SideTab::Contacts => self.requests_in.iter().filter(|r| !self.hidden_requests.contains(&r.did)).count() as u32,
                _ => 0,
            };
            let mut caption = row![pgi(icon, 20.0, color), semi(label, 15.0, color).line_height(lh(21.0)).width(Fill)].spacing(12).height(44).align_y(Alignment::Center);
            if badge > 0 {
                caption = caption.push(t_badge(t, badge, false));
            }
            // The current item's own hover stays transparent so the gliding highlight shows through.
            items = items.push(button(caption).width(Fill).height(44).padding([0, 12])
                .style(wash(t, None, (!on).then_some(t.surface2), 14.0))
                .on_press(Msg::ShellSearch(Action::Tab(tab))));
        }
        let y = self.fx.nav.at(self.fx.now);
        // The stack takes its size from this first layer: all four rows.
        let highlight = column![Space::new().height(y), container(Space::new()).width(Fill).height(44).style(ui::plain(t.accent_soft, 14.0))]
            .height(NAV_STEP * tabs().len() as f32 - 2.0);
        let nav = stack![highlight, items];
        let first = self.profile_name.split_whitespace().next().unwrap_or(&self.profile_name).to_string();
        let me = button(row![self.profile_avatar(t, &self.profile_name, "me", 36.0),
                column![bold(first, 15.0, t.ink).line_height(lh(21.0)), tx(crate::i18n::tr_str("common_your_account"), 13.0, t.ink2).line_height(lh(21.0))]]
                .spacing(10).align_y(Alignment::Center))
            .padding(8).width(Fill).style(wash(t, None, None, 14.0)).on_press(Msg::OpenSettings);
        row![container(column![brand, nav, Space::new().height(Fill), me].spacing(8))
                .padding([16, 12]).width(231).height(Fill),
            container(Space::new()).width(1).height(Fill).style(ui::plain(t.hairline, 0.0))].width(232).height(Fill).into()
    }

    /// DesktopField + SearchField: 40px pill on --surface-2; focus turns it --surface with a 2px
    /// inset accent ring. The clear button keeps its place and only fades, so typing never moves text.
    pub(super) fn desktop_search_field(&self, t: Tok) -> El<'_> {
        let input = text_input(crate::i18n::tr_str("shell_search_ctrl_k"), &self.search).id(SEARCH_ID).on_input(Msg::SearchIn)
            .padding(Padding { top: 9.5, right: 42.0, bottom: 9.5, left: 44.0 }).size(15).line_height(lh(21.0)).font(ui::SANS).width(Fill)
            .style(move |_, status| {
                let focused = matches!(status, iced::widget::text_input::Status::Focused { .. });
                iced::widget::text_input::Style {
                    background: iced::Background::Color(if focused { t.surface } else { t.surface2 }),
                    border: Border { color: if focused { t.accent } else { Color::TRANSPARENT }, width: if focused { 2.0 } else { 0.0 }, radius: 20.0.into() },
                    icon: t.ink2, placeholder: t.ink2, value: t.ink, selection: ui::alpha(t.accent, 0.3),
                }
            });
        let fade = self.fx.clear.at(self.fx.now);
        let clear = button(container(pgi("x", 18.0, ui::alpha(t.ink2, t.ink2.a * fade))).width(36).height(36).center_x(36).center_y(36))
            .padding(0).width(36).height(36).style(wash(t, None, (!self.search.is_empty()).then_some(t.hairline), 18.0))
            .on_press_maybe((!self.search.is_empty()).then_some(Msg::SearchIn(String::new())));
        let over = row![container(pgi("search", 20.0, t.ink2)).width(44).height(40).center_x(44).center_y(40), Space::new().width(Fill),
            container(clear).padding(Padding { right: 4.0, left: 2.0, ..Padding::ZERO }).height(40).center_y(40)];
        container(stack![input, over].height(40))
            .padding(Padding { top: 12.0, right: 12.0, bottom: 4.0, left: 12.0 }).width(Fill).into()
    }

    /// `.listpane header` (16 20 4) with the 24px/21px title.
    pub(super) fn list_header<'a>(&self, t: Tok, title: &'a str) -> El<'a> {
        container(bold(title, 24.0, t.ink).line_height(lh(21.0))).padding(Padding { top: 16.0, right: 20.0, bottom: 4.0, left: 20.0 }).width(Fill).into()
    }

    /// ChatsList `.chips`: All / Unread filter chips and, on desktop, the round New chat button.
    pub(super) fn chat_chips(&self, t: Tok) -> El<'_> {
        let unread = self.shell_search.unread_only;
        let chip = |label: &'static str, on: bool, v: bool| -> El<'_> {
            let style = move |_: &iced::Theme, status: iced::widget::button::Status| {
                let hover = matches!(status, iced::widget::button::Status::Hovered);
                iced::widget::button::Style {
                    background: on.then_some(iced::Background::Color(t.accent_soft)).or(hover.then_some(iced::Background::Color(t.surface2))),
                    text_color: if on { t.accent_ink } else { t.ink },
                    border: Border { color: if on { Color::TRANSPARENT } else { t.line }, width: 1.0, radius: 16.0.into() },
                    ..Default::default()
                }
            };
            button(container(semi(label, 13.0, if on { t.accent_ink } else { t.ink }).line_height(lh(21.0))).height(30).center_y(30))
                .padding(Padding { top: 1.0, right: 15.0, bottom: 1.0, left: 15.0 }).height(32).style(style).on_press(Msg::ShellSearch(Action::Unread(v))).into()
        };
        let new_chat = button(container(pgi("plus", 20.0, t.accent_ink)).width(36).height(36).center_x(36).center_y(36))
            .padding(0).width(36).height(36).style(wash(t, Some(t.accent_soft), Some(ui::mix(t.accent_soft, Color::BLACK, 0.03)), 18.0))
            .on_press(Msg::ShellSearch(Action::NewChat(true)));
        // With no chats at all the filters mean nothing yet: only the + stays (desktop).
        let chips: El = if self.has_chats() { row![chip(crate::i18n::tr_str("shell_all"), !unread, false), chip(crate::i18n::tr_str("shell_unread"), unread, true)].spacing(8).into() } else { Space::new().height(32).into() };
        container(row![chips, Space::new().width(Fill), new_chat].spacing(8).align_y(Alignment::Center))
            .padding(Padding { top: 4.0, right: 16.0, bottom: 8.0, left: 16.0 }).width(Fill).into()
    }

    pub(super) fn shell_search_results(&self, t: Tok) -> El<'_> {
        let heading = |caption| container(label(t, caption)).height(36).padding([0, 16]).center_y(36);
        let result = self.shell_search.result.as_ref();
        let mut tiles = row![].spacing(2);
        if let Some(result) = result {
            for hit in &result.names {
                let face: El = if hit.kind == p2pcore::SearchNameKind::Group { ui::group_face(t, &hit.id, 48.0) } else { self.profile_avatar(t, &hit.name, &hit.id, 48.0) };
                tiles = tiles.push(button(column![face,
                    container(tx(hit.name.clone(), 12.0, t.ink)).height(30).width(70).clip(true)]
                    .spacing(6).align_x(Alignment::Center)).width(78).height(92).padding([6, 4])
                    .style(ui::button_style(t, Kind::Ghost, 14.0)).on_press(Msg::ShellSearch(Action::OpenName(hit.clone()))));
            }
        }
        let names: El = if result.is_some_and(|r| !r.names.is_empty()) {
            scroll(t, tiles).direction(iced::widget::scrollable::Direction::Horizontal(Default::default())).height(100).into()
        } else { container(tx(if self.search.trim().is_empty() { crate::i18n::tr_str("shell_search_messages_people_and") } else { crate::i18n::tr_str("shell_no_people_or_groups") }, 13.0, t.ink2)).height(100).center_x(Fill).center_y(100).into() };
        let mut messages = column![].spacing(0);
        if let Some(result) = result {
            for hit in &result.messages {
                let parts = super::super::shell_search::marked_parts(&hit.snippet, &hit.highlights);
                let spans = parts.into_iter().map(|(s, found)| span::<(), iced::Font>(s).font(if found { ui::SANS_BOLD } else { ui::SANS }).color(if found { t.ink } else { t.ink2 })).collect::<Vec<_>>();
                let title = row![container(semi(hit.conv_name.clone(), 15.0, t.ink).wrapping(iced::widget::text::Wrapping::None)).width(Fill).clip(true),
                    container(tx(hit.day.clone(), 11.0, t.ink2)).width(72)].spacing(8);
                // Message results are deliberately read-only until the chat/group jump-and-highlight hook lands.
                // Do not wire a misleading latest-message navigation as if it were an exact hit jump.
                let face: El = if hit.kind == p2pcore::SearchConvKind::Group { ui::group_face(t, &hit.conv, 44.0) } else { self.profile_avatar(t, &hit.conv_name, &hit.conv, 44.0) };
                messages = messages.push(container(row![face,
                    column![title, container(rich_text(spans).size(13).wrapping(iced::widget::text::Wrapping::None)).height(18).width(Fill).clip(true)].spacing(2).width(Fill)]
                    .spacing(12).align_y(Alignment::Center)).height(64).padding([10, 8]).width(Fill));
            }
        }
        let hint = if let Some(error) = &self.shell_search.error { error.as_str() }
            else if self.shell_search.busy { crate::i18n::tr_str("shell_searching") }
            else if self.search.trim().chars().count() < 2 && !self.search.is_empty() { crate::i18n::tr_str("shell_type_2_or_more") }
            else if result.is_some_and(|r| r.messages.is_empty()) { crate::i18n::tr_str("shell_no_messages_match") } else { "" };
        let more = button(tx(crate::i18n::tr_str("shell_more_messages"), 13.0, t.primary)).height(44).padding([0, 16])
            .style(ui::button_style(t, Kind::Ghost, 14.0)).on_press_maybe(
                (!self.shell_search.busy && result.is_some_and(|r| r.next.is_some())).then_some(Msg::ShellSearch(Action::More)));
        column![heading(crate::i18n::tr_str("shell_people_groups")), names, heading(crate::i18n::tr_str("common_messages")),
            container(tx(hint, 13.0, t.ink2)).height(44).width(Fill).center_x(Fill).center_y(44),
            scroll(t, messages).height(Fill), more].width(Fill).height(Fill).into()
    }

    /// `.empty`: the still mark (56), 6px gaps, title 15/21 bold ink, line 13/21 muted.
    pub(super) fn shell_empty_detail(&self, t: Tok) -> El<'_> {
        container(column![ui::logo(56.0, t), bold(crate::i18n::tr_str("shell_choose_a_chat"), 15.0, t.ink).line_height(lh(21.0)), tx(crate::i18n::tr_str("shell_messages_stay_on_your"), 13.0, t.ink2).line_height(lh(21.0))]
            .spacing(6).align_x(Alignment::Center)).width(Fill).height(Fill).center_x(Fill).center_y(Fill).style(ui::plain(t.bg, 0.0)).into()
    }
}

/// Layer.svelte `variant="dialog"` on desktop: a centred card (min(440, 100% - 32) wide, --r-sheet,
/// --surface-raised, --shadow-sheet) over a rgb(10 16 14 / .5) scrim that closes it. `open` is the
/// enter progress (0..1): the scrim fades, the card rises the last 8px (layerIn, cubicOut).
pub(super) fn pg_dialog<'a>(t: Tok, open: f32, height: Option<f32>, content: El<'a>, close: Msg) -> El<'a> {
    let rise = 8.0 * (1.0 - open);
    let mut card = container(content).width(Length::Fixed(440.0)).max_width(440.0).clip(true);
    if let Some(h) = height {
        card = card.height(h);
    }
    let card = ui::raised(card, t, ui::Depth::Sheet, ui::sheet_surface(t, 24.0));
    let scrim = iced::widget::mouse_area(container(Space::new()).width(Fill).height(Fill)
        .style(ui::plain(Color { r: 10.0 / 255.0, g: 16.0 / 255.0, b: 14.0 / 255.0, a: 0.5 * open }, 0.0)))
        .on_press(close);
    stack![scrim, container(container(card).padding(Padding { top: rise, ..Padding::ZERO }))
        .padding(16).width(Fill).height(Fill).center_x(Fill).center_y(Fill)].into()
}

impl App {
    /// NewChat.svelte in its Layer: shortcuts, then contacts A-Z. The list area is fixed (540px card)
    /// so filtering never resizes the dialog.
    pub(super) fn new_chat_overlay(&self, t: Tok) -> Option<El<'_>> {
        let q = self.shell_search.new_chat.as_ref()?;
        let needle = q.trim().to_lowercase();
        let find = text_input(crate::i18n::tr_str("shell_find_a_contact"), q).id(super::super::shell_search::NEW_CHAT_ID)
            .on_input(|v| Msg::ShellSearch(Action::NewChatFind(v))).padding(Padding { top: 11.5, right: 14.0, bottom: 11.5, left: 40.0 })
            .size(15).line_height(lh(21.0)).font(ui::SANS)
            .style(move |_, status| {
                let focused = matches!(status, iced::widget::text_input::Status::Focused { .. });
                iced::widget::text_input::Style {
                    background: iced::Background::Color(t.surface2),
                    border: Border { color: if focused { t.accent } else { Color::TRANSPARENT }, width: if focused { 2.0 } else { 0.0 }, radius: 22.0.into() },
                    icon: t.ink2, placeholder: t.ink2, value: t.ink, selection: ui::alpha(t.accent, 0.3),
                }
            });
        let find = stack![find, container(pgi("search", 18.0, t.ink2)).padding(Padding { left: 14.0, ..Padding::ZERO }).height(44).center_y(44)];
        let head = container(column![bold(crate::i18n::tr_str("shell_new_chat"), 17.0, t.ink).line_height(lh(24.0)), find].spacing(12))
            .padding(Padding { top: 18.0, right: 20.0, bottom: 8.0, left: 20.0 });
        let act = |icon: &'static str, title: &'static str, sub: &'static str, msg: Msg| -> El<'_> {
            button(row![container(pgi(icon, 22.0, t.accent_ink)).width(44).height(44).center_x(44).center_y(44).style(ui::plain(t.accent_soft, 22.0)),
                    column![semi(title, 15.0, t.ink).line_height(lh(21.0)), tx(sub, 13.0, t.ink2).line_height(lh(18.0))].spacing(1)]
                    .spacing(12).align_y(Alignment::Center).height(64))
                .padding([0, 20]).width(Fill).style(wash(t, None, Some(t.surface2), 0.0)).on_press(msg).into()
        };
        let mut list = column![].spacing(0);
        if needle.is_empty() {
            list = list.push(act("users", crate::i18n::tr_str("common_new_group"), crate::i18n::tr_str("common_youll_be_the_owner"), Msg::ShellSearch(Action::NewChatGroup)))
                .push(act("userplus", crate::i18n::tr_str("common_add_contact"), crate::i18n::tr_str("shell_scan_a_code_or"), Msg::ShellSearch(Action::NewChatAdd)))
                .push(container(label(t, crate::i18n::tr_str("common_contacts")).font(ui::SANS_BOLD).line_height(lh(16.0))).padding(Padding { top: 14.0, right: 20.0, bottom: 4.0, left: 20.0 }));
        }
        let mut people: Vec<&p2pcore::Contact> = self.contacts.iter()
            .filter(|c| !self.blocks.blocked_by_me(&c.did) && (needle.is_empty() || Self::display(c).to_lowercase().contains(&needle)))
            .collect();
        people.sort_by_key(|c| Self::display(c).to_lowercase());
        let any = !people.is_empty();
        for c in people {
            let name = Self::display(c);
            let status = self.profiles.cache.get(&c.did).and_then(|p| p.fields.get("status")).and_then(serde_json::Value::as_str).unwrap_or("").to_string();
            let has_chat = self.chat.chats.iter().any(|r| r.peer_did == c.did && r.last_activity > 0);
            let sub = if !status.is_empty() || has_chat { status } else { crate::i18n::tr_str("common_no_messages_yet").to_string() };
            list = list.push(button(row![self.profile_avatar(t, &name, &c.did, 44.0),
                    column![semi(name, 15.0, t.ink).line_height(lh(21.0)).wrapping(iced::widget::text::Wrapping::None),
                        tx(sub, 13.0, t.ink2).line_height(lh(18.0)).wrapping(iced::widget::text::Wrapping::None)].spacing(1).width(Fill)]
                    .spacing(12).align_y(Alignment::Center).height(64))
                .padding([0, 20]).width(Fill).style(wash(t, None, Some(t.surface2), 14.0))
                .on_press(Msg::ShellSearch(Action::NewChatPick(c.did.clone()))));
        }
        let none = if any { String::new() } else { crate::i18n::tr_args("shell_no_contact_called", &[("trim", &(q.trim()).to_string())]) };
        list = list.push(container(tx(none, 13.0, t.ink2).line_height(lh(19.0))).padding(Padding { top: 12.0, right: 20.0, bottom: 8.0, left: 20.0 }).width(Fill).center_x(Fill));
        let body = column![head, scroll(t, list).height(Fill)].height(Fill);
        Some(pg_dialog(t, 1.0, Some(540.0), body.into(), Msg::ShellSearch(Action::NewChat(false))))
    }
}

impl App {
    /// ContactsList.svelte: find field, Add contact, contacts A-Z under letter headings, then the
    /// people I blocked (dimmed), then the count. Rows are PersonRow (64px, 44 avatar) inside 8px margins.
    pub(super) fn contacts_list(&self, t: Tok) -> El<'_> {
        use crate::fit::ellipsize;
        let q = self.shell_search.contacts_q.trim().to_lowercase();
        let find = text_input(crate::i18n::tr_str("shell_find_a_contact"), &self.shell_search.contacts_q).on_input(|v| Msg::ShellSearch(Action::ContactsFind(v)))
            .padding(Padding { top: 11.5, right: 14.0, bottom: 11.5, left: 40.0 }).size(15).line_height(lh(21.0)).font(ui::SANS)
            .style(move |_, status| {
                let focused = matches!(status, iced::widget::text_input::Status::Focused { .. });
                iced::widget::text_input::Style {
                    background: iced::Background::Color(t.surface2),
                    border: Border { color: if focused { t.accent } else { Color::TRANSPARENT }, width: if focused { 2.0 } else { 0.0 }, radius: 22.0.into() },
                    icon: t.ink2, placeholder: t.ink2, value: t.ink, selection: ui::alpha(t.accent, 0.3),
                }
            });
        let find = container(stack![find, container(pgi("search", 18.0, t.ink2)).padding(Padding { left: 14.0, ..Padding::ZERO }).height(44).center_y(44)])
            .padding(Padding { top: 4.0, right: 16.0, bottom: 8.0, left: 16.0 });
        let add = button(row![container(pgi("userplus", 22.0, t.accent_ink)).width(44).height(44).center_x(44).center_y(44).style(ui::plain(t.accent_soft, 22.0)),
                column![semi(crate::i18n::tr_str("common_add_contact"), 15.0, t.ink).line_height(lh(21.0)), tx(crate::i18n::tr_str("shell_share_your_code_or"), 13.0, t.ink2).line_height(lh(18.0))].spacing(1)]
                .spacing(12).align_y(Alignment::Center).height(64))
            .padding([0, 8]).width(Fill).style(wash(t, None, Some(t.surface2), 14.0)).on_press(Msg::OpenAdd);
        let mut col = column![find, container(add).padding([0, 8])].spacing(0);
        let matches = |c: &&p2pcore::Contact| q.is_empty() || Self::display(c).to_lowercase().contains(&q) || c.name.to_lowercase().contains(&q);
        let mut mine: Vec<&p2pcore::Contact> = self.contacts.iter().filter(|c| !self.blocks.blocked_by_me(&c.did)).filter(matches).collect();
        mine.sort_by_key(|c| Self::display(c).to_lowercase());
        let mut blocked: Vec<&p2pcore::Contact> = self.contacts.iter().filter(|c| self.blocks.blocked_by_me(&c.did)).filter(matches).collect();
        blocked.sort_by_key(|c| Self::display(c).to_lowercase());
        let total = self.contacts.iter().filter(|c| !self.blocks.blocked_by_me(&c.did)).count();
        let sec = |s: String| container(bold(s, 12.0, t.ink2).line_height(lh(16.0))).padding(Padding { top: 18.0, right: 16.0, bottom: 6.0, left: 16.0 });
        let open = match &self.profiles.page {
            Some(super::super::profile_ui::Page::Person(d)) if self.screen == Screen::Settings => Some(d.as_str()),
            _ => None,
        };
        let row_ = |c: &p2pcore::Contact, dim: bool| -> El<'_> {
            let name = Self::display(c);
            let sub = if dim { crate::i18n::tr_str("common_blocked").to_string() } else { self.profiles.cache.get(&c.did).and_then(|p| p.fields.get("status")).and_then(serde_json::Value::as_str).unwrap_or("").to_string() };
            // 380 - 16 list margin - 16 row padding - 44 avatar - 12 gap
            let w = 380.0 - 1.0 - 32.0 - 44.0 - 12.0;
            let fade = |c: Color| if dim { ui::alpha(c, 0.6) } else { c };
            let sel = open == Some(c.did.as_str());
            let face: El = if dim { container(self.profile_avatar(t, &name, &c.did, 44.0)).into() } else { self.profile_avatar(t, &name, &c.did, 44.0) };
            button(row![face, column![semi(ellipsize(&name, ui::SANS_SEMI, 15.0, w), 15.0, fade(t.ink)).line_height(lh(21.0)).wrapping(iced::widget::text::Wrapping::None),
                    tx(ellipsize(&sub, ui::SANS, 13.0, w), 13.0, t.ink2).line_height(lh(18.0)).wrapping(iced::widget::text::Wrapping::None)].spacing(1).width(Fill)]
                    .spacing(12).align_y(Alignment::Center).height(64))
                .padding([0, 8]).width(Fill).style(wash(t, sel.then_some(t.accent_soft), (!sel).then_some(t.surface2), 14.0))
                .on_press(Msg::ShellSearch(Action::OpenPerson(c.did.clone()))).into()
        };
        // People waiting for my answer (cards: Accept / Not now), then requests I sent.
        let waiting: Vec<&p2pcore::ContactRequest> = self.requests_in.iter().filter(|r| !self.hidden_requests.contains(&r.did)).collect();
        if !waiting.is_empty() {
            col = col.push(sec(crate::i18n::tr_str("shell_waiting_for_you").into()));
            let btn = |label_: &'static str, primary: bool, on: Msg| -> El<'static> {
                let style = move |_: &Theme, st: button::Status| {
                    let hover = matches!(st, button::Status::Hovered);
                    let bg = if primary { if hover { ui::mix(t.accent, Color::WHITE, 0.08) } else { t.accent } } else if hover { t.surface2 } else { Color::TRANSPARENT };
                    button::Style { background: Some(iced::Background::Color(bg)), text_color: if primary { t.on_accent } else { t.ink }, border: Border { radius: 22.0.into(), ..Default::default() }, ..Default::default() }
                };
                button(container(semi(label_, 15.0, if primary { t.on_accent } else { t.ink }).line_height(lh(20.0))).center_y(44)).padding([0, 18]).height(44).style(style).on_press(on).into()
            };
            let mut cards = column![].spacing(12);
            for r in waiting {
                let busy = self.add_ui.answering;
                let accept: El = if busy { container(semi(crate::i18n::tr_str("common_accept"), 15.0, t.on_accent)).padding([12, 18]).style(ui::plain(ui::alpha(t.accent, 0.45), 22.0)).into() } else { btn(crate::i18n::tr_str("common_accept"), true, Msg::Gate(super::super::signup::Gs::CardAccept(r.did.clone()))) };
                let card = column![
                    row![avatar(t, &r.name, &r.did, 44.0), column![semi(r.name.clone(), 15.0, t.ink).line_height(lh(21.0)), tx(crate::i18n::tr_str("shell_wants_to_add_you"), 13.0, t.ink2).line_height(lh(18.0))].spacing(1).width(Fill)]
                        .spacing(12)
                        .align_y(Alignment::Center),
                    row![accept, btn(crate::i18n::tr_str("common_not_now"), false, Msg::Gate(super::super::signup::Gs::CardNotNow(r.did.clone())))].spacing(8),
                    tx(crate::i18n::tr_str("shell_accepting_lets_you_message"), 13.0, t.ink2).line_height(lh(19.0)),
                ]
                .spacing(10);
                cards = cards.push(container(card).padding(14).width(Fill).style(move |_: &Theme| container::Style {
                    background: Some(iced::Background::Color(t.surface)),
                    border: Border { color: t.hairline, width: 1.0, radius: 14.0.into() },
                    ..Default::default()
                }));
            }
            col = col.push(container(cards).padding([6, 8]));
        }
        if !self.requests_out.is_empty() {
            // No "declined": an unanswered request just waits (kind to both).
            col = col.push(sec(crate::i18n::tr_str("shell_waiting_for_them").into()));
            let mut g = column![].spacing(0);
            for x in &self.requests_out {
                let f = super::super::signup::first_name(&x.name);
                let w = 380.0 - 1.0 - 32.0 - 40.0 - 12.0;
                g = g.push(container(row![avatar(t, &x.name, &x.did, 40.0), column![semi(ellipsize(&x.name, ui::SANS_SEMI, 15.0, w), 15.0, t.ink).line_height(lh(21.0)).wrapping(iced::widget::text::Wrapping::None),
                    tx(crate::i18n::tr_args("common_request_sent_your_line", &[("name", &(f).to_string())]), 13.0, t.ink2).line_height(lh(18.0))].spacing(1).width(Fill)]
                    .spacing(12).align_y(Alignment::Center)).padding([12, 8]).width(Fill));
            }
            col = col.push(container(g).padding([0, 8]));
        }
        let mut letter = String::new();
        let mut group = column![].spacing(0);
        for c in &mine {
            let l = Self::display(c).chars().next().map(|ch| ch.to_uppercase().collect::<String>()).unwrap_or_else(|| "#".into());
            if l != letter {
                if !letter.is_empty() {
                    col = col.push(container(group).padding([0, 8]));
                    group = column![].spacing(0);
                }
                col = col.push(sec(l.clone()));
                letter = l;
            }
            group = group.push(row_(c, false));
        }
        if !letter.is_empty() {
            col = col.push(container(group).padding([0, 8]));
        }
        if !blocked.is_empty() {
            col = col.push(sec(crate::i18n::tr_str("shell_blocked").into()));
            let mut g = column![].spacing(0);
            for c in &blocked {
                g = g.push(row_(c, true));
            }
            col = col.push(container(g).padding([0, 8]));
        }
        if total == 0 && blocked.is_empty() && self.requests_out.is_empty() && q.is_empty() {
            // EmptyLine `contacts`: replaces "0 contacts"; the Add contact row above is the action.
            return col.push(self.empty_quiet(t, crate::i18n::tr_str("shell_the_people_you_add"))).into();
        }
        let foot = if total == 0 && q.is_empty() {
            String::new()
        } else if !q.is_empty() && mine.is_empty() && blocked.is_empty() {
            crate::i18n::tr_args("shell_no_one_called_in", &[("trim", &(self.shell_search.contacts_q.trim()).to_string())])
        } else {
            crate::i18n::tr_count("shell_contacts_count", total as u64, &[])
        };
        col = col.push(container(tx(foot, 13.0, t.ink2).line_height(lh(19.0))).padding(Padding { top: 14.0, right: 16.0, bottom: 24.0, left: 16.0 }).width(Fill).center_x(Fill));
        col.into()
    }
}
