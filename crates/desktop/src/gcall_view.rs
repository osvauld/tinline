//! GroupCallScreen.svelte (desktop), GroupCallStrip.svelte and OngoingCall.svelte, over the core's group calls
//! (`app::gcall`). Every text row keeps its height when its words change, as in the playground.
use super::*;
use iced::widget::{column, stack};
use crate::app::gcall::{Gm, Phase};

fn gm(m: Gm) -> Msg {
    Msg::GCall(m)
}

impl App {
    /// A person in a group call: "You", their name in the group, a contact's name, else "Someone".
    pub(super) fn gcall_name(&self, did: &str) -> String {
        if self.is_me(did) {
            return "You".into();
        }
        if let Some(m) = self.live.detail.as_ref().and_then(|d| d.members.iter().find(|m| m.did == did)) {
            return m.name.clone();
        }
        self.contacts.iter().find(|c| c.did == did).map(Self::display).unwrap_or_else(|| "Someone".into())
    }

    fn gcall_group_name(&self) -> String {
        let gid = self.gcall.gid.clone().unwrap_or_default();
        self.live.list.iter().find(|s| s.id == gid).map(|s| s.name.clone()).unwrap_or_else(|| "Group call".into())
    }

    /// gcall.svelte.ts `hostLabel`: "this computer" or "Arjun's device" (the core doesn't say what kind of device).
    fn gcall_host_label(&self) -> Option<String> {
        if self.gcall.hosting {
            return Some("this computer".into());
        }
        let did = self.gcall.host.as_ref()?;
        let n = self.gcall_name(did);
        Some(format!("{}\u{2019}s device", n.split_whitespace().next().unwrap_or(&n)))
    }

    /// The full group call screen (not minimised).
    pub(super) fn gcall_view(&self, t: Tok) -> El<'_> {
        let g = &self.gcall;
        let live = matches!(g.phase, Phase::Starting | Phase::Connecting | Phase::Active | Phase::Reconnecting);
        let connected = matches!(g.phase, Phase::Active | Phase::Reconnecting);
        let btn = |icon: &'static str, size: f32, msg: Option<Msg>| -> El<'_> {
            match msg {
                Some(m) => button(container(crate::pg_icons::pgi(icon, size, t.ink)).center_x(44).center_y(44)).padding(0).width(44).height(44)
                    .style(shell_view::wash(t, None, Some(ui::alpha(t.ink, 0.1)), 22.0)).on_press(m).into(),
                None => Space::new().width(44).height(44).into(),
            }
        };
        let top: El = container(row![
            btn("chev", 24.0, live.then_some(gm(Gm::Minimize))),
            container(ui::logo_live(28.0, t, self.mark_look())).center_x(Fill),
            // On the host's own screen there is no line to measure, so the header keeps the "i" (§ 6h); clients have no
            // line data from the core yet either.
            btn("info", 22.0, (live && g.host.is_some() || g.hosting).then_some(gm(Gm::About(true)))),
        ].align_y(Alignment::Center)).height(48).into();
        // .head: name 26/32 650, then the status (or the timer), the count and the speaking line, each in a fixed row.
        let n = g.parts.len();
        let count = if connected && n > 0 {
            if n == 1 { "Only you so far. Waiting for others.".to_string() } else { format!("{n} people in the call") }
        } else { String::new() };
        let status_cell: El = if connected && g.phase == Phase::Active {
            mono(format!("{:02}:{:02}", g.elapsed() / 60, g.elapsed() % 60), 20.0, t.ink).line_height(lh(30.0)).into()
        } else {
            semi(g.status(), 17.0, t.ink).line_height(lh(24.0)).into()
        };
        let head = column![
            container(semi(self.gcall_group_name(), 26.0, t.ink).line_height(lh(32.0)).wrapping(iced::widget::text::Wrapping::None)).width(Fill).center_x(Fill).clip(true),
            container(status_cell).height(30).center_x(Fill).center_y(30),
            container(tx(count, 13.0, t.ink).line_height(lh(20.0))).height(20).center_x(Fill),
            Space::new().height(20),
        ].spacing(4).align_x(Alignment::Center).padding(Padding { top: 6.0, ..Padding::ZERO });
        // .roster: tiles in a centred grid, sized by how many there are (desktop: up to 4 → 112, up to 8 → 96, else 84).
        let tiles = n.min(12) + usize::from(n > 12);
        let (cols, av) = if tiles <= 4 { (tiles.max(1), 112.0) } else if tiles <= 8 { (4, 96.0) } else { (4, 84.0) };
        let mut grid = column![].spacing(20).align_x(Alignment::Center);
        let shown: Vec<&String> = g.parts.iter().take(if n > 12 { 11 } else { 12 }).collect();
        let dim = !matches!(g.phase, Phase::Active | Phase::Connecting | Phase::Starting);
        let face = |did: &str| -> El<'_> {
            let name = if self.is_me(did) { self.profile_name.clone() } else { self.gcall_name(did) };
            let first = if self.is_me(did) { "You".to_string() } else { name.split_whitespace().next().unwrap_or(&name).to_string() };
            column![self.profile_avatar(t, &name, did, av), container(semi(first, 13.0, t.ink).line_height(lh(18.0)).wrapping(iced::widget::text::Wrapping::None)).width(av + 24.0).center_x(av + 24.0).clip(true)]
                .spacing(8).align_x(Alignment::Center).width(Length::Fixed(av + 40.0)).into()
        };
        for chunk in shown.chunks(cols) {
            let mut r = row![].spacing(0).align_y(Alignment::Start);
            for did in chunk {
                r = r.push(face(did));
            }
            grid = grid.push(r);
        }
        if n > 12 {
            let more = n - 11;
            grid = grid.push(column![container(bold(format!("+{more}"), (av * 0.3).round(), t.ink)).width(av).height(av).center_x(av).center_y(av).style(ui::plain(ui::mix(t.surface, t.ink, 0.11), av / 2.0)),
                semi("more", 13.0, t.ink)].spacing(8).align_x(Alignment::Center));
        }
        let grid = container(grid).padding([34, 28]).max_width(760).center_x(Fill);
        let mut stage = stack![container(grid).width(Fill).height(Fill).center_y(Fill).style(move |_: &Theme| container::Style { text_color: None, ..Default::default() })];
        let _ = dim;
        if let Some(note) = &g.note {
            let card = container(row![crate::pg_icons::pgi("battery", 18.0, t.warn_ink), tx(note.clone(), 13.0, t.warn_ink).line_height(lh(18.0)).width(Fill),
                button(container(bold("OK", 13.0, t.warn_ink)).center_y(36)).padding([0, 14]).height(36)
                    .style(move |_: &Theme, _| button::Style { background: Some(ui::alpha(t.warn_ink, 0.14).into()), border: Border { radius: 18.0.into(), ..Default::default() }, ..Default::default() })
                    .on_press(gm(Gm::NoteOk))].spacing(10).align_y(Alignment::Center))
                .padding(Padding { top: 8.0, right: 8.0, bottom: 8.0, left: 14.0 }).width(Fill);
            stage = stage.push(container(ui::raised(card, t, ui::Depth::Pop, ui::plain(t.warn, 18.0))).width(Fill).height(Fill).align_bottom(Fill).padding(Padding { bottom: 4.0, ..Padding::ZERO }));
        }
        // .host: "Call host: Arjun's device" with the info icon; it opens About this call.
        let host_text = self.gcall_host_label().map(|h| if g.hosting { "You\u{2019}re the call host".to_string() } else { format!("Call host: {h}") });
        let host: El = match host_text.filter(|_| connected || g.phase == Phase::Connecting) {
            Some(h) => button(row![tx(h, 13.0, t.ink).line_height(lh(20.0)), crate::pg_icons::pgi("info", 16.0, ui::alpha(t.ink, 0.75))].spacing(8).align_y(Alignment::Center))
                .padding([0, 14]).height(44).style(shell_view::wash(t, None, Some(ui::alpha(t.ink, 0.09)), 22.0)).on_press(gm(Gm::About(true))).into(),
            None => Space::new().height(44).into(),
        };
        let host = container(host).center_x(Fill).padding(Padding { top: 4.0, bottom: 12.0, ..Padding::ZERO });
        let ctl = row![
            self.round_btn(t, Kind::Quiet, if g.muted { Icon::MicOff } else { Icon::Mic }, if g.muted { "Muted" } else { "Mute" }, connected.then_some(gm(Gm::Mute)), g.muted),
            self.round_btn(t, Kind::End, Icon::PhoneOff, "Leave", live.then_some(gm(Gm::Leave)), false),
        ].spacing(56);
        let content = column![top, head, stage.height(Fill), host, container(ctl).center_x(Fill)]
            .padding(Padding { top: 8.0, right: 20.0, bottom: 36.0, left: 20.0 }).width(Fill).height(Fill);
        let mut layers = stack![container(content).width(Fill).height(Fill).style(ui::plain(t.bg, 0.0))];
        if g.about {
            layers = layers.push(self.gcall_about(t));
        }
        layers.into()
    }

    /// About this call (group): who carries the call and what that means; honest about the host hearing it.
    fn gcall_about(&self, t: Tok) -> El<'_> {
        let host = self.gcall_host_label().unwrap_or_else(|| "The call host".into());
        let cap = { let mut c = host.chars(); c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default() };
        let p = |s: String| -> El<'_> { tx(s, 13.0, t.ink2).line_height(lh(19.0)).into() };
        let lead = iced::widget::rich_text![
            iced::widget::span::<(), iced::Font>(cap).font(ui::SANS_SEMI).color(t.ink),
            iced::widget::span::<(), iced::Font>(format!(" {} this call. Everyone in the call connects to it, and it passes the audio on and chooses who is heard when several people speak.", if self.gcall.hosting { "is carrying" } else { "carries" })).color(t.ink2),
        ].size(13).line_height(lh(19.0));
        let got = button(container(semi("Got it", 15.0, t.on_primary)).center_x(Fill).center_y(44)).padding(0).width(Fill).height(44)
            .style(ui::button_style(t, Kind::Primary, 22.0)).on_press(gm(Gm::About(false)));
        let content = column![
            semi("About this call", 17.0, t.ink).line_height(lh(24.0)),
            Space::new().height(2),
            lead,
            p("Your connection to the call host is encrypted, but the call host itself receives the audio, so it can hear this call. You trust it for the call the way you trust a chat host with posts.".into()),
            p("A call host is a device a group admin chose and that agreed to it. It is separate from chat hosting. Admins can change call hosts in Group info.".into()),
            Space::new().height(8),
            got,
        ].spacing(8);
        let card = crate::shaped::shaped(container(content).padding(Padding { top: 20.0, right: 20.0, bottom: 16.0, left: 20.0 }).width(420), [24.0; 4], t.raised);
        let card = ui::raised(card, t, ui::Depth::Sheet, ui::plain(Color::TRANSPARENT, 24.0));
        let scrim = button(Space::new().width(Fill).height(Fill)).padding(0)
            .style(move |_, _| button::Style { background: Some(ui::alpha(Color::BLACK, 0.24).into()), ..Default::default() })
            .on_press(gm(Gm::About(false)));
        stack![scrim, container(card).center_x(Fill).align_bottom(Fill).padding(24)].into()
    }

    /// GroupCallStrip.svelte: the minimised group call, the same strip as a 1:1 call (users icon, Leave).
    pub(super) fn gcall_strip(&self, t: Tok) -> Option<El<'_>> {
        let g = &self.gcall;
        if !g.live() || !g.minimized {
            return None;
        }
        let st = if g.phase == Phase::Active { format!("{:02}:{:02}", g.elapsed() / 60, g.elapsed() % 60) } else { g.status() };
        let open = button(row![crate::pg_icons::pgi("users", 18.0, t.on_accent),
                container(semi(self.gcall_group_name(), 15.0, t.on_accent).wrapping(iced::widget::text::Wrapping::None)).width(Fill).clip(true),
                tx(st, 13.0, t.on_accent)].spacing(10).align_y(Alignment::Center).height(44))
            .padding([0, 12]).height(44).width(Fill).style(|_: &Theme, _| button::Style::default()).on_press(gm(Gm::Restore));
        let end = button(container(bold("Leave", 13.0, t.on_danger_btn)).center_y(32)).padding([0, 14]).height(32)
            .style(move |_: &Theme, _| button::Style { background: Some(t.danger_btn.into()), border: Border { radius: 16.0.into(), ..Default::default() }, ..Default::default() })
            .on_press(gm(Gm::Leave));
        Some(container(row![open, end].spacing(8).align_y(Alignment::Center)).padding(Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 4.0 })
            .height(44).width(Fill).style(ui::plain(t.accent, 0.0)).into())
    }

    /// OngoingCall.svelte: "Call in progress" under a group's header while its call runs and I'm not in it (Join),
    /// or "You're in the call" with the timer (Open). The core doesn't say how many joined a call I'm not in, so the
    /// line under it names who started it.
    pub(super) fn gcall_banner(&self, t: Tok, gid: &str) -> Option<El<'_>> {
        let mine = self.gcall.gid.as_deref() == Some(gid) && self.gcall.live();
        let c = self.gcall.live.get(gid);
        if !mine && c.is_none() {
            return None;
        }
        let (title, sub) = if mine {
            ("You\u{2019}re in the call", if self.gcall.phase == Phase::Active { format!("{:02}:{:02}", self.gcall.elapsed() / 60, self.gcall.elapsed() % 60) } else { self.gcall.status() })
        } else {
            let by = c.map(|c| self.gcall_name(&c.starter_did)).unwrap_or_default();
            ("Call in progress", format!("Started by {}", by.split_whitespace().next().unwrap_or(&by)))
        };
        let fg = if t.dark { t.ink } else { t.accent_ink };
        let bg = if t.dark { ui::mix(t.surface, t.accent, 0.24) } else { t.accent_soft };
        let j = if mine {
            button(container(bold("Open", 13.0, t.accent_ink)).center_x(Fill).center_y(36)).padding([0, 18]).width(Length::Fixed(80.0)).height(36)
                .style(move |_: &Theme, _| button::Style { border: Border { color: t.accent_ink, width: 1.5, radius: 18.0.into() }, ..Default::default() })
                .on_press(gm(Gm::Restore))
        } else {
            button(container(bold("Join", 13.0, t.on_accent)).center_x(Fill).center_y(36)).padding([0, 18]).width(Length::Fixed(80.0)).height(36)
                .style(move |_: &Theme, _| button::Style { background: Some(t.accent.into()), border: Border { radius: 18.0.into(), ..Default::default() }, ..Default::default() })
                .on_press(gm(Gm::Call(gid.to_string())))
        };
        Some(container(row![crate::pg_icons::pgi("users", 20.0, fg),
                column![semi(title, 15.0, fg).line_height(lh(20.0)), tx(sub, 13.0, fg).line_height(lh(17.0))].width(Fill), j]
                .spacing(12).align_y(Alignment::Center))
            .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 14.0 }).height(56).width(Fill).style(ui::plain(bg, 0.0)).into())
    }
}
