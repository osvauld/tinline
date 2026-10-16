//! 1:1 voice surface from CallScreen.svelte. No group-call or media policy here.
use super::*;
use iced::widget::column;
use crate::call_ui::{Action, Geometry};

fn top_button<'a>(t: Tok, icon: Icon, caption: &'static str, action: Option<Msg>) -> El<'a> {
    let b = button(container(ui::icon(icon, 24.0, t.ink)).center_x(44).center_y(44))
        .padding(0).width(44).height(44).style(ui::button_style(t, Kind::Ghost, 999.0)).on_press_maybe(action);
    iced::widget::tooltip(b, caption, iced::widget::tooltip::Position::Bottom).into()
}
fn at<'a>(content: impl Into<El<'a>>, y: f32) -> El<'a> {
    container(content).padding(Padding { top: y.max(0.0), ..Padding::ZERO }).width(Fill).into()
}

impl App {
    pub(super) fn call_view(&self, t: Tok) -> El<'_> {
        let c = self.call.as_ref().unwrap();
        if c.state == CallState::Active && self.video.live() {
            // Keep real camera frames; this milestone does not replace the video stage.
            return self.video_call_view(t);
        }
        iced::widget::responsive(move |size| {
            let g = Geometry::new(size.height);
            let incoming = c.info.incoming && !c.answered && c.state == CallState::Ringing;
            let active = c.state == CallState::Active;
            let reconnecting = active && c.stats.as_ref().is_some_and(|s| s.reconnecting);
            let secs = c.stats.as_ref().map_or(0, |s| s.secs);
            let status = if incoming { "Incoming call".to_string() }
                else if reconnecting { "Reconnecting…".into() }
                else { match c.state {
                    CallState::Dialing => "Calling…".into(),
                    CallState::Ringing => "Ringing…".into(),
                    CallState::Active => format!("{:02}:{:02}", secs / 60, secs % 60),
                    CallState::Ended { .. } => "Call ended".into(),
                }};
            let path = c.stats.as_ref().map(|s| if s.direct { "Direct" } else { "Relayed" });
            let sub = if reconnecting { format!("{:02}:{:02}", secs / 60, secs % 60) }
                else if active { path.map_or("End-to-end encrypted".into(), |p| format!("End-to-end encrypted · {p}")) } else { String::new() };
            // .min.gone: minimize and the bars only while the call is live (not incoming, not ended); the slots stay.
            let live = !incoming && !matches!(c.state, CallState::Ended { .. });
            let (level, _) = self.signal_now();
            let info_label = if reconnecting { "About this call. Reconnecting.".to_string() }
                else if level > 0 { format!("About this call. {} signal, {}.", crate::call_ui::SIGNAL_WORD[level as usize], path.unwrap_or("Relayed").to_lowercase()) }
                else { "About this call".into() };
            let bars = button(container(ui::signal_bars(self.fx.bar_alphas(), 24.0, t.ink)).center_x(44).center_y(44))
                .padding(0).width(44).height(44).style(ui::button_style(t, Kind::Ghost, 999.0)).on_press(Msg::CallUi(Action::About));
            let slot = |e: El<'static>| -> El<'static> { if live { e } else { Space::new().width(44).height(44).into() } };
            let head: El = container(row![
                slot(top_button(t, Icon::ChevronDown, "Minimize call", Some(Msg::CallUi(Action::Minimize)))),
                container(ui::logo_live(28.0, t, self.mark_look())).center_x(Fill),
                slot(iced::widget::tooltip(bars, text(info_label).size(13), iced::widget::tooltip::Position::Bottom).into()),
            ].align_y(Alignment::Center)).height(48).padding([0, 20]).into();
            // Fixed line boxes; the timer, encryption and mute labels never alter sibling geometry.
            let who: El = column![
                self.profile_avatar(t, &c.info.peer_name, &c.info.peer_did, 128.0),
                Space::new().height(22),
                container(semi(c.info.peer_name.clone(), 28.0, t.ink).wrapping(iced::widget::text::Wrapping::None))
                    .height(34).width(Fill).center_x(Fill).clip(true),
                Space::new().height(6),
                container(if active && !reconnecting { mono(status, 20.0, t.ink) } else { semi(status, 17.0, t.ink) })
                    .height(30).center_x(Fill).center_y(30),
                Space::new().height(10),
                container(tx(sub, 13.0, t.ink)).height(18).center_x(Fill),
            ].align_x(Alignment::Center).width(Fill).into();
            let muted = self.ctl.muted.load(Ordering::Relaxed);
            let controls: El = if incoming {
                row![
                    self.round_btn(t, Kind::End, Icon::Hangup, "Decline", Some(Msg::Decline), false),
                    self.round_btn(t, Kind::Accept, Icon::Phone, "Answer", Some(Msg::Answer), false),
                ].spacing(36).into()
            } else {
                row![
                    self.round_btn(t, Kind::Quiet, if muted { Icon::MicOff } else { Icon::Mic }, if muted { "Muted" } else { "Mute" }, active.then_some(Msg::ToggleMute), muted),
                    self.round_btn(t, Kind::End, Icon::Hangup, "End", Some(Msg::Hangup), false),
                    self.round_btn(t, Kind::Quiet, if self.video.local_on() { Icon::Video } else { Icon::VideoOff },
                        if matches!(self.video.source, Some(None)) { "No camera" } else { "Camera" },
                        (active && self.video.negotiated() && self.video.has_camera()).then_some(Msg::Video(super::super::Vm::ToggleCamera)), self.video.local_on()),
                ].spacing(56).into()
            };
            let controls = container(controls).center_x(Fill);
            let mut layers = stack![Space::new().width(Fill).height(Fill)];
            // The call field (CloudField.svelte): the Fog cloud behind the call, as on Android. No synth audio is played.
            if let Some(texture) = crate::call_field::texture() {
                use crate::call_field::{State, Want};
                let state = if self.waiting.is_some() { State::Waiting } else if incoming { State::Incoming }
                    else if reconnecting { State::Reconnecting } else if active { State::Active }
                    else if c.state == CallState::Dialing { State::Outgoing } else { State::Connecting };
                layers = layers.push(crate::call_field::field(Want {
                    state, dark: t.dark, muted,
                    avatar: (size.width / 2.0, g.avatar_top + 64.0),
                    calm: [(size.width / 2.0, g.text_top + 49.0, (size.width - 40.0).min(420.0) / 2.0, 49.0),
                        (size.width / 2.0, g.controls_top + 45.0, 170.0, 45.0)],
                }, texture));
            }
            layers = layers.push(at(head, 8.0)).push(at(container(who).padding([0, 20]), g.avatar_top))
                .push(at(controls, g.controls_top));
            // Waiting is overlaid, not inserted: the active avatar/status/controls stay still.
            if let Some(w) = &self.waiting {
                // WaitingCard: bottom edge 22 px above the controls (150 px tall: 14 + 40 + 12 + 44 + 12 + 16 + 12).
                layers = layers.push(at(container(self.waiting_banner(t, w)).padding([0, 20]), (g.controls_top - 22.0 - 150.0).max(56.0)));
            }
            if self.notice.is_some() {
                layers = layers.push(at(container(self.notice_bar(t)).padding([0, 20]), 58.0));
            }
            if self.call_ui.about {
                layers = layers.push(self.call_about(t));
            }
            layers.width(Fill).height(Fill).into()
        }).into()
    }

    /// The bars' level (0 = ghosts) and whether the reconnecting wave runs (SignalBars in CallScreen).
    pub(super) fn signal_now(&self) -> (u8, bool) {
        let Some(c) = self.call.as_ref().filter(|c| c.state == CallState::Active) else { return (0, false) };
        match c.stats.as_ref() {
            Some(st) if st.reconnecting => (0, true),
            Some(st) => (crate::call_ui::strength(st.rtt_ms, self.call_ui.loss.pct()), false),
            None => (0, false),
        }
    }

    /// CallScreen.svelte `.info`: encryption, then the connection row (bars + "Strong signal · Direct" and one
    /// line, in a block that reserves title + four lines so the card never grows), then the numbers.
    fn call_about(&self, t: Tok) -> El<'_> {
        let c = self.call.as_ref().unwrap();
        let active = c.state == CallState::Active;
        let name = c.info.peer_name.clone();
        let first = name.split_whitespace().next().unwrap_or(&name).to_string();
        let p = |s: String| -> iced::widget::Text<'_> { tx(s, 13.0, t.ink2).line_height(lh(19.0)) };
        let mut content = column![semi("About this call", 17.0, t.ink).line_height(lh(24.0)), Space::new().height(10)];
        if let Some(st) = c.stats.as_ref().filter(|_| active) {
            content = content.push(iced::widget::rich_text![
                iced::widget::span::<(), iced::Font>("End-to-end encrypted.").font(ui::SANS_SEMI).color(t.ink),
                iced::widget::span::<(), iced::Font>(format!(" Your voice is encrypted on this device and only decrypted on {name}’s.")).color(t.ink2),
            ].size(13).line_height(lh(19.0)));
            let (level, wave) = self.signal_now();
            let title = if wave { "Reconnecting".to_string() }
                else { format!("{} signal · {}", crate::call_ui::SIGNAL_WORD[level as usize], if st.direct { "Direct" } else { "Relayed" }) };
            let mut line = if wave { format!("The connection dropped for a moment. Tinline is finding a way back to {first}.") }
                else if st.direct { format!("Your call goes straight between your device and {first}’s.") }
                else { "Your devices can’t reach each other directly right now, so a relay passes the call along. It can’t listen.".into() };
            if !wave && level == 1 { line += &format!(" The line is weak, so {first}’s voice may break up."); }
            let conn = row![
                container(ui::signal_bars(self.fx.bar_alphas(), 22.0, t.ink)).padding(Padding { top: 1.0, ..Padding::ZERO }),
                container(column![semi(title, 13.0, t.ink).line_height(lh(19.0)), p(line)]).width(Fill).height(95),
            ].spacing(12);
            content = content.push(Space::new().height(12)).push(conn);
            if !wave {
                content = content.push(container(tx(format!("{} ms round trip · {}% lost", st.rtt_ms, crate::call_ui::pct(self.call_ui.loss.pct())), 11.0, t.ink2))
                    .padding(Padding { top: 6.0, left: 34.0, ..Padding::ZERO }));
            }
            // Codec per direction while that camera is on. Both rows stay on a video call (blank when off), so nothing moves.
            if self.video.negotiated() {
                for (who, on, what) in [("Your video", self.video.local_on(), &self.video.send_codec), ("Their video", self.video.remote_on(), &self.video.recv_codec)] {
                    let what = if on { what.clone() } else { String::new() };
                    content = content.push(container(row![container(tx(who, 11.0, t.ink2)).width(88), tx(what, 11.0, t.ink)].spacing(12))
                        .padding(Padding { top: 6.0, left: 34.0, ..Padding::ZERO }).height(25));
                }
            }
            // Desktop keeps the real device selectors here, off the focus screen. No immediate-switch claim.
            for (title, devices, selected, on) in [
                ("Microphone", &self.devices.0, &self.settings.input, Msg::InDev as fn(String) -> Msg),
                ("Speaker", &self.devices.1, &self.settings.output, Msg::OutDev as fn(String) -> Msg),
            ] {
                let mut opts = vec![DEFAULT_LABEL.to_string()]; opts.extend(devices.iter().cloned());
                content = content.push(Space::new().height(12)).push(column![tx(title, 13.0, t.ink2),
                    pick_list(opts, Some(selected.clone().unwrap_or_else(|| DEFAULT_LABEL.into())), on)
                        .width(Fill).padding(12).text_size(14).style(ui::pick_style(t)).menu_style(ui::menu_style(t))].spacing(6));
            }
            content = content.push(Space::new().height(6)).push(tx("Device changes apply from the next call.", 12.0, t.ink2));
        } else {
            content = content.push(p(format!("Connecting to {name}. Once the call is connected, it is end-to-end encrypted.")));
        }
        // .b: full width, 44 tall, radius 22.
        let got = button(container(semi("Got it", 15.0, t.on_primary)).center_x(Fill).center_y(44)).padding(0).width(Fill).height(44)
            .style(ui::button_style(t, Kind::Primary, 22.0)).on_press(Msg::CallUi(Action::CloseAbout));
        content = content.push(Space::new().height(16)).push(got);
        let card = crate::shaped::shaped(container(content).padding(Padding { top: 20.0, right: 20.0, bottom: 16.0, left: 20.0 }).width(420), [24.0; 4], t.raised);
        let card = ui::raised(card, t, ui::Depth::Sheet, ui::plain(Color::TRANSPARENT, 24.0));
        let scrim = button(Space::new().width(Fill).height(Fill)).padding(0)
            .style(move |_, _| button::Style { background: Some(ui::alpha(Color::BLACK, 0.24).into()), ..Default::default() })
            .on_press(Msg::CallUi(Action::CloseAbout));
        stack![scrim, container(card).center_x(Fill).align_bottom(Fill).padding(24)].into()
    }

    /// CallStrip.svelte: a minimised call, 44 px across the top of the window (in flow, so the
    /// app below moves down with it): accent, phone 18, the name 15/650, the timer (or status) 13 tabular at the end,
    /// and End as a 32 px danger pill. Tapping the strip returns to the call.
    pub(super) fn call_strip(&self, t: Tok) -> Option<El<'_>> {
        let c = self.call.as_ref().filter(|_| self.call_ui.minimized)?;
        let secs = c.stats.as_ref().map_or(0, |s| s.secs);
        let status = if c.state == CallState::Active { format!("{:02}:{:02}", secs / 60, secs % 60) }
            else if c.state == CallState::Dialing { "Calling…".into() } else { "Ringing…".into() };
        let open = button(row![crate::pg_icons::pgi("phone", 18.0, t.on_accent),
                container(semi(c.info.peer_name.clone(), 15.0, t.on_accent).wrapping(iced::widget::text::Wrapping::None)).width(Fill).clip(true),
                container(tx(status, 13.0, t.on_accent)).width(Length::Shrink)].spacing(10).align_y(Alignment::Center).height(44))
            .padding([0, 12]).height(44).width(Fill).style(|_: &Theme, _| button::Style::default())
            .on_press(Msg::CallUi(Action::Restore));
        let end = button(container(bold("End", 13.0, t.on_danger_btn)).center_y(32)).padding([0, 14]).height(32)
            .style(move |_: &Theme, _| button::Style { background: Some(t.danger_btn.into()), border: Border { radius: 16.0.into(), ..Default::default() }, ..Default::default() })
            .on_press(Msg::Hangup);
        Some(container(row![open, end].spacing(8).align_y(Alignment::Center)).padding(Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 4.0 })
            .height(44).width(Fill).style(ui::plain(t.accent, 0.0)).into())
    }

    /// The desktop frame: strip over (navigation | list | pane). The minimised call is the window's top bar, full width
    /// over the navigation too, like the phone's strip at the top (DesktopShell.svelte `.callstrip`, 2026-10-16); a
    /// waiting call turns the brand row into the caller (ring_head_waiting).
    pub(super) fn framed<'a>(&'a self, t: Tok, pane: El<'a>) -> El<'a> {
        let mut col = column![];
        if let Some(s) = self.call_strip(t) { col = col.push(s); }
        if let Some(s) = self.gcall_strip(t) { col = col.push(s); }
        col.push(row![self.shell_navigation(t), self.list_pane(t), pane].height(Fill)).width(Fill).height(Fill).into()
    }
}
