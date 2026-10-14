//! Live vector preview of the verified audio simulation, not a production call UI.
//! Opt-in default microphone; no audio saved/sent, speakers, profile access or MP4 playback.
#![allow(dead_code)]
#[path = "../../../tools/audio-motion-sim/fixture.rs"]
mod fixture;
#[path = "../../../tools/audio-motion-sim/mic.rs"]
mod mic;
#[path = "../../../tools/audio-motion-sim/motion.rs"]
mod motion;
#[path = "../../gcall-bench/src/spectral.rs"]
mod spectral;
#[path = "../src/ui.rs"]
mod ui;
// ui.rs draws the app's icons and mark, so the example carries those modules too.
#[path = "../src/mark.rs"]
#[allow(dead_code)]
mod mark;
#[path = "../../tin-icons/generated/tin_icons.rs"]
#[allow(dead_code)]
mod tin_icons;

use iced::{
    Color, Element, Length, Rectangle, Renderer, Subscription, Theme,
    widget::{button, canvas, column, container, row, slider, text},
};
use motion::{CONTOURS, Model, Point as MotionPoint, TRAIL};
use std::{
    cell::Cell,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Input {
    Timeline,
    Low,
    High,
    Mix,
    Microphone,
}
#[derive(Clone, Debug)]
enum Msg {
    Tick,
    Pause,
    Replay,
    Dark,
    Reduced,
    Input(Input),
    Contrast(f32),
    Microphone,
}
struct Demo {
    model: Model,
    extractors: [spectral::Extractor; 4],
    sample: u64,
    paused: bool,
    dark: bool,
    input: Input,
    contrast: f32,
    aspect: Cell<f64>,
    update_us: f64,
    draw_us: [Cell<f64>; 2],
    mic: Option<mic::Mic>,
    note: String,
    last_tick: Instant,
}
impl Default for Demo {
    fn default() -> Self {
        Self {
            model: Model::new(),
            extractors: std::array::from_fn(|_| spectral::Extractor::new()),
            sample: 0,
            paused: false,
            dark: true,
            input: Input::Timeline,
            contrast: 1.7,
            aspect: Cell::new(1.2),
            update_us: 0.0,
            draw_us: std::array::from_fn(|_| Cell::new(0.0)),
            mic: None,
            note: "Microphone off — start explicitly; no audio saved or sent".into(),
            last_tick: Instant::now(),
        }
    }
}
impl Demo {
    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Tick if !self.paused => {
                let start = Instant::now();
                self.model.aspect = self.aspect.get();
                match self.input {
                    Input::Timeline => {
                        fixture::synthetic(&mut self.model, &mut self.extractors, &mut self.sample)
                    }
                    Input::Microphone => {
                        if self.mic.as_ref().is_some_and(|m| m.failed()) {
                            self.mic = None;
                            self.model.end_call();
                            self.note =
                                "Microphone stream error — stopped; check input device and retry"
                                    .into();
                        }
                        if let Some(mic) = &mut self.mic {
                            if let Some(o) = mic.poll() {
                                let at = self.model.time() - o.at.elapsed().as_secs_f64();
                                if let Some(f) = o.features {
                                    self.model.feed_spectrum(
                                        0,
                                        o.level,
                                        o.audible,
                                        false,
                                        at,
                                        motion::ShapeFeatures {
                                            low: f.low,
                                            high: f.high,
                                            centroid_hz: f.centroid_hz,
                                            onset: f.onset,
                                        },
                                    );
                                } else {
                                    self.model.feed(0, o.level, o.audible, false, at);
                                }
                                self.note = format!(
                                    "Default mic live · level {} (-dBov) · dropped blocks {} · no audio saved/sent",
                                    o.level,
                                    mic.drops()
                                );
                            } else if !mic.fresh() {
                                self.model.feed(0, 127, false, false, self.model.time());
                            }
                        } else {
                            self.model.feed(0, 127, false, false, self.model.time());
                        }
                    }
                    i => fixture::tone(
                        &mut self.model,
                        &mut self.extractors[0],
                        &mut self.sample,
                        i != Input::High,
                        i != Input::Low,
                    ),
                }
                let dt = if self.input == Input::Microphone {
                    self.last_tick.elapsed().as_secs_f64().min(0.1)
                } else {
                    1.0 / 30.0
                };
                self.last_tick = Instant::now();
                self.model.step(dt);
                self.update_us = start.elapsed().as_secs_f64() * 1e6;
                if self.input == Input::Timeline && self.model.time() >= 20.0 {
                    self.paused = true;
                }
            }
            Msg::Pause => {
                self.paused = !self.paused;
                self.last_tick = Instant::now();
                if self.paused && self.input == Input::Microphone {
                    self.mic = None;
                    self.model.end_call();
                    self.note =
                        "Microphone stopped by Pause — click Start microphone to capture again"
                            .into();
                }
            }
            Msg::Replay => {
                if self.input == Input::Microphone {
                    self.mic = None;
                    self.input = Input::Timeline;
                    self.note = "Microphone off — replaying synthetic scenario".into();
                }
                let reduced = self.model.is_reduced();
                self.model = Model::new();
                self.model.set_reduced(reduced);
                for e in &mut self.extractors {
                    e.reset();
                }
                self.sample = 0;
                self.paused = false;
                self.last_tick = Instant::now();
            }
            Msg::Dark => self.dark = !self.dark,
            Msg::Reduced => self.model.set_reduced(!self.model.is_reduced()),
            Msg::Input(input) => {
                self.mic = None;
                self.note = "Microphone off — synthetic input".into();
                self.input = input;
                if input == Input::Timeline {
                    self.update(Msg::Replay);
                    return;
                }
                self.model.start_call();
                self.model.set_roster(&[0, 1, 2, 3]);
                // Input change never resets visual phase/pool. Replay is the explicit reset.
                self.paused = false;
            }
            Msg::Microphone => {
                if self.mic.take().is_some() {
                    self.model.end_call();
                    self.note = "Microphone stopped — no audio saved or sent".into();
                } else {
                    match mic::Mic::start() {
                        Ok(mic) => {
                            self.mic = Some(mic);
                            self.input = Input::Microphone;
                            self.model.start_call();
                            self.model.set_roster(&[0]);
                            self.paused = false;
                            self.last_tick = Instant::now();
                            self.note =
                                "Default microphone starting — no audio saved or sent".into();
                        }
                        Err(e) => {
                            self.note = format!("Microphone not started: {e}");
                        }
                    }
                }
            }
            Msg::Contrast(c) => self.contrast = c,
            _ => {}
        }
    }
    fn pane(&self, particles: bool) -> Element<'_, Msg> {
        let index = usize::from(particles);
        column![
            text(if particles {
                "B / Persistent flowing marks"
            } else {
                "A / Soft curved field"
            })
            .size(17),
            canvas(Plot {
                model: &self.model,
                particles,
                dark: self.dark,
                contrast: self.contrast,
                aspect: &self.aspect,
                draw_us: &self.draw_us[index]
            })
            .width(Length::Fill)
            .height(Length::Fill)
        ]
        .spacing(12)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
    fn view(&self) -> Element<'_, Msg> {
        let stage = match self.input {
            Input::Low => "200 Hz tone / level 35",
            Input::High => "4 kHz tone / level 35",
            Input::Mix => "200 Hz + 4 kHz / same nominal RMS",
            Input::Microphone => "Live microphone / local PCM -> spectrum -> motion",
            Input::Timeline => match self.model.time() {
                t if t < 2.0 => "Silence",
                t if t < 4.0 => "Low-frequency phrases",
                t if t < 6.0 => "High-frequency phrases / same level",
                t if t < 7.0 => "Pause",
                t if t < 8.0 => "Loud onset",
                t if t < 12.0 => "Audible overlap",
                t if t < 14.0 => "Muted",
                t if t < 16.0 => "Stale input",
                t if t < 18.0 => "Roster change",
                _ => "Call ended / fade",
            },
        };
        container(column![
            text("Audio motion / live simulation").size(24),
            text("Alexandria Montgomery-Wellington · native Iced Canvas · no video compression").size(13),
            row![button(if self.paused { "Resume" } else { "Pause" }).width(100).on_press(Msg::Pause),
                button("Replay").width(100).on_press(Msg::Replay), button(if self.dark { "Light theme" } else { "Dark theme" }).width(140).on_press(Msg::Dark),
                button(if self.model.is_reduced() { "Motion: reduced" } else { "Motion: continuous" }).width(200).on_press(Msg::Reduced)].spacing(12),
            row![button("20s scenario").on_press(Msg::Input(Input::Timeline)),
                button("200 Hz").on_press(Msg::Input(Input::Low)), button("4 kHz").on_press(Msg::Input(Input::High)),
                button("Mixture").on_press(Msg::Input(Input::Mix)), text("Preview contrast"),
                slider(0.5..=3.0, self.contrast, Msg::Contrast).width(180)].spacing(12),
            row![button(if self.mic.is_some() { "Stop microphone" } else { "Start microphone" })
                .width(190).on_press(Msg::Microphone), text(&self.note).size(13)].spacing(12),
            row![text(format!("Time {:05.2}s", self.model.time())).width(190), text(stage)].spacing(12),
            row![self.pane(false), self.pane(true)].spacing(24).height(Length::Fill),
            text(format!("Update CPU {:.0} µs · Canvas CPU A/B {:.0}/{:.0} µs · GPU/layout excluded",
                self.update_us, self.draw_us[0].get(), self.draw_us[1].get())).size(12),
            text("Mic is opt-in; Stop/Pause/input switch/window close stops capture · no audio files, playback, account or network").size(12),
        ].spacing(14)).padding(24).width(Length::Fill).height(Length::Fill).into()
    }
    fn subscription(&self) -> Subscription<Msg> {
        if self.paused {
            Subscription::none()
        } else {
            iced::time::every(Duration::from_micros(33_333)).map(|_| Msg::Tick)
        }
    }
}

struct Plot<'a> {
    model: &'a Model,
    particles: bool,
    dark: bool,
    contrast: f32,
    aspect: &'a Cell<f64>,
    draw_us: &'a Cell<f64>,
}
impl canvas::Program<Msg> for Plot<'_> {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let start = Instant::now();
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        self.aspect
            .set((bounds.width / bounds.height.max(1.0)).max(0.01) as f64);
        let point = |p: MotionPoint| {
            iced::Point::new(p.x as f32 * bounds.width, p.y as f32 * bounds.height)
        };
        let mut color = if self.dark {
            Color::from_rgb8(215, 187, 130)
        } else {
            Color::from_rgb8(146, 114, 63)
        };
        let opacity = (self.model.opacity() as f32 * self.contrast).min(0.65);
        let mut stroke = |path: &canvas::Path| {
            for (width, gain) in [(7.0, 0.10), (1.8, 1.0)] {
                color.a = opacity * gain;
                frame.stroke(
                    path,
                    canvas::Stroke::default()
                        .with_color(color)
                        .with_width(width)
                        .with_line_cap(canvas::LineCap::Round)
                        .with_line_join(canvas::LineJoin::Round),
                );
            }
        };
        if !self.particles {
            for i in 0..CONTOURS {
                let curve = self.model.contour(i);
                let path = canvas::Path::new(|b| {
                    b.move_to(point(curve[0].start));
                    for c in curve {
                        b.bezier_curve_to(point(c.c1), point(c.c2), point(c.end));
                    }
                });
                stroke(&path);
            }
        } else {
            for p in &self.model.particles {
                let first = (p.head + TRAIL - p.trail_len) % TRAIL;
                let path = canvas::Path::new(|b| {
                    for j in 0..p.trail_len {
                        let current = p.trail[(first + j) % TRAIL];
                        let previous = p.trail[(first + j.saturating_sub(1)) % TRAIL];
                        if j == 0 || (current.x - previous.x).abs() > 0.4 {
                            b.move_to(point(current));
                        } else {
                            let mut before = p.trail[(first + j.saturating_sub(2)) % TRAIL];
                            let mut next = p.trail[(first + (j + 1).min(p.trail_len - 1)) % TRAIL];
                            if (before.x - previous.x).abs() > 0.4 {
                                before = previous;
                            }
                            if (next.x - current.x).abs() > 0.4 {
                                next = current;
                            }
                            b.bezier_curve_to(
                                point(MotionPoint {
                                    x: previous.x + (current.x - before.x) / 6.0,
                                    y: previous.y + (current.y - before.y) / 6.0,
                                }),
                                point(MotionPoint {
                                    x: current.x - (next.x - previous.x) / 6.0,
                                    y: current.y - (next.y - previous.y) / 6.0,
                                }),
                                point(current),
                            );
                        }
                    }
                });
                stroke(&path);
            }
        }
        let geometry = frame.into_geometry();
        self.draw_us.set(start.elapsed().as_secs_f64() * 1e6);
        vec![geometry]
    }
}
fn main() -> iced::Result {
    iced::application(Demo::default, Demo::update, Demo::view)
        .title("Tinline — audio motion simulation")
        .theme(|d: &Demo| ui::theme(d.dark))
        .font(ui::FIGTREE_BYTES)
        .font(ui::PLEX_REGULAR_BYTES)
        .font(ui::PLEX_MEDIUM_BYTES)
        .default_font(ui::SANS)
        .subscription(Demo::subscription)
        .window_size((1160.0, 740.0))
        .run()
}
