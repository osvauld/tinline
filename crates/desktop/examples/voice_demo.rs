//! Try the voice message widgets without chat: `cargo run -p desktop --example voice_demo`.
//! Click the mic to record, Send (or the mic again) to finish, Esc or the bin to cancel; each
//! recording becomes a bubble to play, pause and seek. Needs a microphone and a speaker.

#![allow(dead_code)]

#[path = "../src/ui.rs"]
mod ui;
#[path = "../src/voice.rs"]
mod voice;

use std::path::PathBuf;

use iced::widget::{column, container, row, scrollable, text, Space};
use iced::{keyboard, Element, Length, Subscription, Task};
use p2pcore::VoiceInfo;

#[derive(Clone, Debug)]
enum Msg {
    Mic,
    Send,
    Cancel,
    Toggle(usize),
    Seek(usize, f32),
    Tick,
    Key(keyboard::Key),
}

struct Item {
    path: PathBuf,
    info: VoiceInfo,
    outgoing: bool,
}

#[derive(Default)]
struct Demo {
    rec: Option<voice::Recorder>,
    elapsed: u32,
    items: Vec<Item>,
    playing: Option<(usize, voice::Player)>,
    note: String,
}

impl Demo {
    fn path(n: usize) -> PathBuf {
        std::env::temp_dir().join(format!("tinline-demo-{n}.opus"))
    }

    fn update(&mut self, m: Msg) -> Task<Msg> {
        match m {
            Msg::Mic => match &self.rec {
                Some(_) => return self.update(Msg::Send),
                None => match voice::Recorder::start(&Self::path(self.items.len())) {
                    Ok(r) => {
                        self.playing = None;
                        self.rec = Some(r);
                        self.elapsed = 0;
                        self.note.clear();
                    }
                    Err(e) => self.note = e,
                },
            },
            Msg::Send => {
                if let Some(r) = self.rec.take() {
                    let n = self.items.len();
                    match r.finish() {
                        Ok(Some(info)) => self.items.push(Item { path: Self::path(n), info, outgoing: n.is_multiple_of(2) }),
                        Ok(None) => self.note = "Too short, discarded".into(),
                        Err(e) => self.note = e,
                    }
                }
            }
            Msg::Cancel => {
                if let Some(r) = self.rec.take() {
                    r.cancel();
                }
            }
            Msg::Key(k) => {
                if k == keyboard::Key::Named(keyboard::key::Named::Escape) {
                    return self.update(Msg::Cancel);
                }
            }
            Msg::Toggle(i) => match &self.playing {
                Some((j, p)) if *j == i => {
                    if p.is_paused() || p.finished() {
                        p.resume()
                    } else {
                        p.pause()
                    }
                }
                _ => match voice::Player::play(&self.items[i].path) {
                    Ok(p) => self.playing = Some((i, p)),
                    Err(e) => self.note = e,
                },
            },
            Msg::Seek(i, f) => {
                let ms = (f * self.items[i].info.duration_ms as f32) as u32;
                match &self.playing {
                    Some((j, p)) if *j == i => p.seek(ms),
                    _ => {
                        if let Ok(p) = voice::Player::play(&self.items[i].path) {
                            p.seek(ms);
                            self.playing = Some((i, p));
                        }
                    }
                }
            }
            Msg::Tick => {
                if let Some(r) = &self.rec {
                    self.elapsed = r.elapsed_ms();
                    if self.elapsed >= voice::MAX_MS {
                        return self.update(Msg::Send);
                    }
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Msg> {
        let t = ui::LIGHT;
        let mut list = column![].spacing(6).padding(12);
        for (i, it) in self.items.iter().enumerate() {
            let (pos, playing) = match &self.playing {
                Some((j, p)) if *j == i => (Some(p.position_ms()), !p.is_paused() && !p.finished()),
                _ => (None, false),
            };
            let v = voice::VoiceView { waveform: &it.info.waveform, duration_ms: it.info.duration_ms, position_ms: pos, playing };
            let b = container(voice::bubble(t, v, it.outgoing, Msg::Toggle(i), move |f| Msg::Seek(i, f)))
                .padding(10)
                .style(ui::plain(if it.outgoing { t.primary_c } else { t.surface }, 16.0));
            list = list.push(if it.outgoing { row![Space::new().width(Length::Fill), b] } else { row![b] });
        }
        let bottom: Element<Msg> = match &self.rec {
            Some(_) => voice::record_bar(t, self.elapsed, Msg::Send, Msg::Cancel),
            None => row![text(self.note.clone()).width(Length::Fill), voice::mic_button(t, Msg::Mic)].spacing(8).into(),
        };
        container(column![scrollable(list).height(Length::Fill), container(bottom).padding(8)])
            .width(Length::Fill)
            .height(Length::Fill)
            .style(ui::plain(t.bg, 0.0))
            .into()
    }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            iced::time::every(std::time::Duration::from_millis(100)).map(|_| Msg::Tick),
            keyboard::listen().filter_map(|e| match e {
                keyboard::Event::KeyPressed { key, .. } => Some(Msg::Key(key)),
                _ => None,
            }),
        ])
    }
}

fn main() -> iced::Result {
    iced::application(Demo::default, Demo::update, Demo::view)
        .title("Tinline voice demo")
        .theme(|_: &Demo| ui::theme(false))
        .font(ui::FIGTREE_BYTES)
        .font(ui::PLEX_REGULAR_BYTES)
        .font(ui::PLEX_MEDIUM_BYTES)
        .default_font(ui::SANS)
        .subscription(Demo::subscription)
        .window_size((420.0, 640.0))
        .run()
}
