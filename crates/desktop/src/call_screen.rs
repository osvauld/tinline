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
            let sub = if reconnecting { format!("{:02}:{:02}", secs / 60, secs % 60) }
                else if active { "End-to-end encrypted".into() } else { String::new() };
            let head: El = container(row![
                top_button(t, Icon::ChevronDown, "Minimize call", (!incoming).then_some(Msg::CallUi(Action::Minimize))),
                container(ui::logo_live(28.0, t, self.mark_look())).center_x(Fill),
                top_button(t, Icon::Info, "About this call", Some(Msg::CallUi(Action::About))),
            ].align_y(Alignment::Center)).height(48).padding([0, 20]).into();
            // Fixed line boxes; the timer, encryption and mute labels never alter sibling geometry.
            let who: El = column![
                avatar(t, &c.info.peer_name, &c.info.peer_did, 128.0),
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
                    self.round_btn(t, Kind::End, Icon::PhoneOff, "Decline", Some(Msg::Decline), false),
                    self.round_btn(t, Kind::Accept, Icon::Phone, "Answer", Some(Msg::Answer), false),
                ].spacing(36).into()
            } else {
                row![
                    self.round_btn(t, Kind::Quiet, if muted { Icon::MicOff } else { Icon::Mic }, if muted { "Muted" } else { "Mute" }, active.then_some(Msg::ToggleMute), muted),
                    self.round_btn(t, Kind::End, Icon::PhoneOff, "End", Some(Msg::Hangup), false),
                    self.round_btn(t, Kind::Quiet, if self.video.local_on() { Icon::Video } else { Icon::VideoOff }, "Camera",
                        (active && self.video.negotiated() && self.video.has_camera()).then_some(Msg::Video(super::super::Vm::ToggleCamera)), self.video.local_on()),
                ].spacing(56).into()
            };
            let controls = container(controls).center_x(Fill);
            let mut layers = stack![Space::new().width(Fill).height(Fill)];
            // Explicit test-hooks preview only. No texture is chosen for production, no synth audio is played.
            if self.demo && let Some(texture) = crate::call_field::texture() {
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
                layers = layers.push(at(container(self.waiting_banner(t, w)).padding([0, 20]), (g.controls_top - 110.0).max(56.0)));
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

    fn call_about(&self, t: Tok) -> El<'_> {
        let c = self.call.as_ref().unwrap();
        let active = c.state == CallState::Active;
        let text = if active {
            format!("End-to-end encrypted. Your voice is encrypted on this device and only decrypted on {}’s. When a relay helps connect you, it passes the call along but can’t listen.", c.info.peer_name)
        } else {
            format!("Connecting to {}. Once the call is connected, it is end-to-end encrypted.", c.info.peer_name)
        };
        let mut content = column![semi("About this call", 17.0, t.ink), tx(text, 13.0, t.ink2)].spacing(10);
        if let Some(st) = c.stats.as_ref().filter(|_| active) {
            let total = st.received + st.lost;
            let loss = if total > 0 { st.lost as f64 * 100.0 / total as f64 } else { 0.0 };
            let (_, quality) = quality(st.rtt_ms, loss);
            content = content.push(tx(format!("{} · {quality}", if st.direct { "Direct" } else { "Relayed" }), 13.0, t.ink2));
            // Retain the real device selectors, moved off the focus screen. No immediate-switch claim.
            for (title, devices, selected, on) in [
                ("Microphone", &self.devices.0, &self.settings.input, Msg::InDev as fn(String) -> Msg),
                ("Speaker", &self.devices.1, &self.settings.output, Msg::OutDev as fn(String) -> Msg),
            ] {
                let mut opts = vec![DEFAULT_LABEL.to_string()]; opts.extend(devices.iter().cloned());
                content = content.push(column![tx(title, 13.0, t.ink2),
                    pick_list(opts, Some(selected.clone().unwrap_or_else(|| DEFAULT_LABEL.into())), on)
                        .width(Fill).padding(12).text_size(14).style(ui::pick_style(t)).menu_style(ui::menu_style(t))].spacing(6));
            }
            content = content.push(tx("Device changes apply from the next call.", 12.0, t.ink2));
        }
        content = content.push(pill(t, Kind::Primary, None, "Got it", Some(Msg::CallUi(Action::CloseAbout))));
        let card = crate::shaped::shaped(container(content).padding(20).width(420), [24.0; 4], t.raised);
        let card = ui::raised(card, t, ui::Depth::Sheet, ui::plain(Color::TRANSPARENT, 24.0));
        let scrim = button(Space::new().width(Fill).height(Fill)).padding(0)
            .style(move |_, _| button::Style { background: Some(ui::alpha(Color::BLACK, 0.24).into()), ..Default::default() })
            .on_press(Msg::CallUi(Action::CloseAbout));
        stack![scrim, container(card).center_x(Fill).align_bottom(Fill).padding(24)].into()
    }

    pub(super) fn call_strip_pane<'a>(&'a self, t: Tok, pane: El<'a>) -> El<'a> {
        let Some(c) = self.call.as_ref().filter(|_| self.call_ui.minimized) else { return pane };
        let secs = c.stats.as_ref().map_or(0, |s| s.secs);
        let status = if c.state == CallState::Active { format!("{:02}:{:02}", secs / 60, secs % 60) }
            else if c.state == CallState::Dialing { "Calling…".into() } else { "Ringing…".into() };
        let open = button(row![ui::logo_live(24.0, t, self.mark_look()),
            container(semi(c.info.peer_name.clone(), 14.0, t.ink).wrapping(iced::widget::text::Wrapping::None)).width(Fill).clip(true),
            container(mono(status, 13.0, t.ink)).width(110)].spacing(8).align_y(Alignment::Center))
            .padding([0, 16]).height(44).width(Fill).style(ui::button_style(t, Kind::Ghost, 0.0))
            .on_press(Msg::CallUi(Action::Restore));
        let end = button(semi("End", 13.0, t.end)).height(44).padding([0, 16])
            .style(ui::button_style(t, Kind::Ghost, 0.0)).on_press(Msg::Hangup);
        let strip = container(row![open, end]).height(44).width(Fill).style(ui::plain(t.primary_c, 0.0));
        let mut body = column![strip];
        if let Some(w) = &self.waiting { body = body.push(self.waiting_banner(t, w)); }
        body.push(pane).width(Fill).height(Fill).into()
    }
}
