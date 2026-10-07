// lead: replace with voice.rs
//! Placeholders for voice messages until `voice.rs` (recorder, player, bubble) lands: a mic
//! button that only explains itself, and a bubble with a static waveform and the duration.

use iced::widget::{button, container, row, text, Space};
use iced::{Alignment, Color, Element, Length};
use p2pcore::Attachment;

use crate::app::Msg;
use crate::ui::{self, Icon, Kind, Tok};

pub fn voice_unavailable() -> &'static str {
    "Voice messages are coming soon on desktop."
}

/// The mic in the composer. lead: swap `on` for the recorder's start message.
pub fn mic_button<'a>(t: Tok, on: Msg) -> Element<'a, Msg> {
    button(container(ui::icon(Icon::Mic, 20.0, t.ink2)).center_x(40).center_y(40))
        .padding(0)
        .width(40)
        .height(40)
        .style(ui::button_style(t, Kind::Ghost, 999.0))
        .on_press(on)
        .into()
}

/// "0:32"
pub fn duration(ms: u32) -> String {
    let s = ms.div_ceil(1000);
    format!("{}:{:02}", s / 60, s % 60)
}

/// A voice bubble's body: play (inert for now), the waveform as bars, the duration.
pub fn voice_bubble<'a>(t: Tok, a: &Attachment, fg: Color) -> Element<'a, Msg> {
    let play = container(ui::icon(Icon::Play, 16.0, t.on_primary)).center_x(34).center_y(34).style(ui::plain(t.primary, 17.0));
    let mut bars = row![].spacing(2).align_y(Alignment::Center);
    let peaks: Vec<u8> = if a.waveform.is_empty() { vec![40; 48] } else { a.waveform.clone() };
    for p in peaks {
        let h = 3.0 + (p as f32 / 255.0) * 22.0;
        bars = bars.push(container(Space::new()).width(2).height(h).style(ui::plain(ui::alpha(fg, 0.55), 1.0)));
    }
    row![play, bars, text(duration(a.duration_ms)).size(13).color(ui::alpha(fg, 0.8)).font(ui::MONO)]
        .spacing(10)
        .align_y(Alignment::Center)
        .width(Length::Shrink)
        .into()
}
