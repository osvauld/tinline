//! Voice messages on the desktop: a cpal recorder and player around the core's Ogg Opus
//! recorder/decoder (16 kHz mono), and the iced widgets for the bubble and the record bar.
//!
//! Recording is click-driven (a mouse cannot hold): click the mic to start, click Send to finish,
//! Esc or the bin to cancel. Nothing here is wired into a screen yet; `cargo run -p desktop
//! --example voice_demo` shows and exercises it all.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig};
use iced::widget::{button, canvas, container, row, text, Space};
use iced::{Alignment, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};
use p2pcore::{VoiceDecoder, VoiceInfo, VoiceRecorder};
use rtrb::RingBuffer;

use crate::ui::{self, Icon, Kind, Tok};

const RATE: u32 = 16_000;
/// Recordings shorter than this are dropped, like on the phone.
pub const MIN_MS: u32 = 500;
/// Longest recording; the caller should finish when `Recorder::elapsed_ms` reaches it.
pub const MAX_MS: u32 = 15 * 60 * 1000;
const BARS: usize = 64;

// ---------------------------------------------------------------------------------------------
// Resampling
// ---------------------------------------------------------------------------------------------

/// Streaming area-average downsampler (a box low-pass), mono f32.
struct Down {
    step: f64,
    acc: f64,
    filled: f64,
}

impl Down {
    fn new(from: u32, to: u32) -> Self {
        Self { step: from as f64 / to as f64, acc: 0.0, filled: 0.0 }
    }

    fn push(&mut self, s: f32, mut emit: impl FnMut(f32)) {
        let mut left = 1.0f64;
        loop {
            let need = self.step - self.filled;
            if left >= need {
                self.acc += s as f64 * need;
                emit((self.acc / self.step) as f32);
                self.acc = 0.0;
                self.filled = 0.0;
                left -= need;
                if left <= 1e-9 {
                    break;
                }
            } else {
                self.acc += s as f64 * left;
                self.filled += left;
                break;
            }
        }
    }
}

/// Streaming linear upsampler.
struct Up {
    step: f64,
    pos: f64,
    last: f32,
}

impl Up {
    fn new(from: u32, to: u32) -> Self {
        Self { step: from as f64 / to as f64, pos: 0.0, last: 0.0 }
    }

    fn process(&mut self, input: &[f32], mut emit: impl FnMut(f32)) {
        if input.is_empty() {
            return;
        }
        let n = input.len() + 1;
        while self.pos + 1.0 < n as f64 {
            let i = self.pos as usize;
            let f = (self.pos - i as f64) as f32;
            let a = if i == 0 { self.last } else { input[i - 1] };
            emit(a * (1.0 - f) + input[i] * f);
            self.pos += self.step;
        }
        self.pos -= (n - 1) as f64;
        self.last = input[n - 2];
    }
}

macro_rules! by_format {
    ($fmt:expr, $f:ident($($arg:expr),*)) => {
        match $fmt {
            SampleFormat::F32 => $f::<f32>($($arg),*),
            SampleFormat::F64 => $f::<f64>($($arg),*),
            SampleFormat::I8 => $f::<i8>($($arg),*),
            SampleFormat::I16 => $f::<i16>($($arg),*),
            SampleFormat::I32 => $f::<i32>($($arg),*),
            SampleFormat::U8 => $f::<u8>($($arg),*),
            SampleFormat::U16 => $f::<u16>($($arg),*),
            SampleFormat::U32 => $f::<u32>($($arg),*),
            other => Err(format!("unsupported sample format {other}")),
        }
    };
}

// ---------------------------------------------------------------------------------------------
// Recorder
// ---------------------------------------------------------------------------------------------

/// A running recording on the default input device. Dropping it without `finish` cancels.
pub struct Recorder {
    _stream: Stream,
    core: Arc<VoiceRecorder>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    done: bool,
}

fn build_in<T>(dev: &cpal::Device, cfg: StreamConfig, mut prod: rtrb::Producer<i16>) -> Result<Stream, String>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let ch = (cfg.channels as usize).max(1);
    let mut down = Down::new(cfg.sample_rate, RATE);
    dev.build_input_stream(
        cfg,
        move |d: &[T], _: &_| {
            for fr in d.chunks_exact(ch) {
                let m = fr.iter().map(|s| f32::from_sample(*s)).sum::<f32>() / ch as f32;
                down.push(m, |o| {
                    let _ = prod.push((o.clamp(-1.0, 1.0) * 32767.0) as i16);
                });
            }
        },
        |e| eprintln!("voice mic: {e}"),
        None,
    )
    .map_err(|e| e.to_string())
}

impl Recorder {
    /// Starts recording into `path` (Ogg Opus). Errors if there is no usable microphone.
    pub fn start(path: &Path) -> Result<Self, String> {
        let dev = cpal::default_host().default_input_device().ok_or("no microphone found")?;
        let c = dev.default_input_config().map_err(|e| e.to_string())?;
        let (fmt, cfg) = (c.sample_format(), c.config());
        let core = VoiceRecorder::start(path.to_string_lossy().into_owned()).map_err(|e| e.to_string())?;
        let (prod, mut cons) = RingBuffer::<i16>::new(RATE as usize * 2);
        let stream = by_format!(fmt, build_in(&dev, cfg, prod)).inspect_err(|_| core.cancel())?;
        stream.play().map_err(|e| e.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let (core, stop) = (core.clone(), stop.clone());
            thread::spawn(move || {
                let mut buf = Vec::new();
                loop {
                    let last = stop.load(Ordering::Relaxed);
                    while let Ok(s) = cons.pop() {
                        buf.push(s);
                    }
                    if !buf.is_empty() {
                        let _ = core.push(std::mem::take(&mut buf));
                    }
                    if last {
                        break;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
            })
        };
        Ok(Self { _stream: stream, core, stop, worker: Some(worker), done: false })
    }

    pub fn elapsed_ms(&self) -> u32 {
        self.core.elapsed_ms()
    }

    fn halt(&mut self) {
        self.done = true;
        self.stop.store(true, Ordering::Relaxed);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }

    /// Stops and closes the file. `Ok(None)` if it was shorter than [`MIN_MS`] (file deleted).
    pub fn finish(mut self) -> Result<Option<VoiceInfo>, String> {
        self.halt();
        let info = self.core.finish().map_err(|e| e.to_string())?;
        Ok(if info.duration_ms < MIN_MS { self.core.cancel(); None } else { Some(info) })
    }

    /// Stops and deletes the file.
    pub fn cancel(mut self) {
        self.halt();
        self.core.cancel();
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        if !self.done {
            self.halt();
            self.core.cancel();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Player
// ---------------------------------------------------------------------------------------------

/// Plays one voice file on the default output device. Dropping it stops playback.
pub struct Player {
    _stream: Stream,
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    seek_to: Arc<AtomicU32>,
    pos_ms: Arc<AtomicU32>,
    duration_ms: u32,
    worker: Option<JoinHandle<()>>,
}

const NO_SEEK: u32 = u32::MAX;

fn build_out<T>(dev: &cpal::Device, cfg: StreamConfig, mut cons: rtrb::Consumer<f32>) -> Result<Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let ch = (cfg.channels as usize).max(1);
    dev.build_output_stream(
        cfg,
        move |d: &mut [T], _: &_| {
            for fr in d.chunks_mut(ch) {
                fr.fill(T::from_sample(cons.pop().unwrap_or(0.0).clamp(-1.0, 1.0)));
            }
        },
        |e| eprintln!("voice speaker: {e}"),
        None,
    )
    .map_err(|e| e.to_string())
}

impl Player {
    /// Opens `path` and starts playing from the beginning.
    pub fn play(path: &Path) -> Result<Self, String> {
        let dec = VoiceDecoder::open(path.to_string_lossy().into_owned()).map_err(|e| e.to_string())?;
        let dev = cpal::default_host().default_output_device().ok_or("no speaker found")?;
        let c = dev.default_output_config().map_err(|e| e.to_string())?;
        let (fmt, cfg) = (c.sample_format(), c.config());
        let rate = cfg.sample_rate;
        let ring = rate as usize / 8;
        let (mut prod, cons) = RingBuffer::<f32>::new(ring);
        let stream = by_format!(fmt, build_out(&dev, cfg, cons))?;
        stream.play().map_err(|e| e.to_string())?;

        let duration_ms = dec.duration_ms();
        let stop = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let seek_to = Arc::new(AtomicU32::new(NO_SEEK));
        let pos_ms = Arc::new(AtomicU32::new(0));
        let worker = {
            let (stop, paused, finished, seek_to, pos_ms) =
                (stop.clone(), paused.clone(), finished.clone(), seek_to.clone(), pos_ms.clone());
            thread::spawn(move || {
                let mut up = Up::new(RATE, rate);
                let mut at_end = false;
                while !stop.load(Ordering::Relaxed) {
                    let s = seek_to.swap(NO_SEEK, Ordering::Relaxed);
                    if s != NO_SEEK {
                        dec.seek(s);
                        at_end = false;
                        finished.store(false, Ordering::Relaxed);
                    }
                    let queued_ms = ((ring - prod.slots()) as u64 * 1000 / rate as u64) as u32;
                    if at_end {
                        if queued_ms == 0 {
                            finished.store(true, Ordering::Relaxed);
                            pos_ms.store(duration_ms, Ordering::Relaxed);
                        }
                        thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                    pos_ms.store(dec.position_ms().saturating_sub(queued_ms), Ordering::Relaxed);
                    if paused.load(Ordering::Relaxed) || prod.slots() < ring / 2 {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    match dec.read(RATE / 50) {
                        Ok(pcm) if !pcm.is_empty() => {
                            let f: Vec<f32> = pcm.iter().map(|&s| s as f32 / 32768.0).collect();
                            up.process(&f, |o| {
                                let _ = prod.push(o);
                            });
                        }
                        _ => at_end = true,
                    }
                }
            })
        };
        Ok(Self { _stream: stream, stop, paused, finished, seek_to, pos_ms, duration_ms, worker: Some(worker) })
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::Relaxed);
    }

    pub fn resume(&self) {
        if self.finished.load(Ordering::Relaxed) {
            self.seek(0);
        }
        self.paused.store(false, Ordering::Relaxed);
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    pub fn seek(&self, ms: u32) {
        self.seek_to.store(ms.min(self.duration_ms), Ordering::Relaxed);
        self.pos_ms.store(ms.min(self.duration_ms), Ordering::Relaxed);
    }

    pub fn position_ms(&self) -> u32 {
        self.pos_ms.load(Ordering::Relaxed)
    }

    pub fn duration_ms(&self) -> u32 {
        self.duration_ms
    }

    /// Played to the end (the host should drop the player or call `resume` to replay).
    pub fn finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Widgets
// ---------------------------------------------------------------------------------------------

pub fn clock(ms: u32) -> String {
    let s = (ms + 500) / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

fn glyph<'a, M: 'a>(body: &'static str, size: f32, color: Color, fill: bool) -> Element<'a, M> {
    let paint = if fill { r##"fill="#000" stroke="none""## } else { ui::STROKE };
    ui::glyph(paint, body, size, color)
}

/// What the bubble shows. `position_ms` is `None` before the first play.
#[derive(Clone, Copy)]
pub struct VoiceView<'a> {
    pub waveform: &'a [u8],
    pub duration_ms: u32,
    pub position_ms: Option<u32>,
    pub playing: bool,
}

struct Wave<'a, M> {
    t: Tok,
    v: VoiceView<'a>,
    outgoing: bool,
    on_seek: Box<dyn Fn(f32) -> M + 'a>,
}

impl<M> canvas::Program<M> for Wave<'_, M> {
    type State = ();

    fn update(
        &self,
        _: &mut (),
        event: &iced::Event,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Option<canvas::Action<M>> {
        if let iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) = event {
            let p = cursor.position_in(bounds)?;
            return Some(canvas::Action::publish((self.on_seek)((p.x / bounds.width).clamp(0.0, 1.0))).and_capture());
        }
        None
    }

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: iced::mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        let (bw, gap) = (3.0f32, 2.0f32);
        let n = (((w + gap) / (bw + gap)) as usize).clamp(8, BARS);
        let progress = match (self.v.position_ms, self.v.duration_ms) {
            (Some(p), d) if d > 0 => p as f32 / d as f32,
            _ => 0.0,
        };
        let idle = if self.outgoing { ui::alpha(self.t.on_primary_c, 0.35) } else { ui::alpha(self.t.ink2, 0.45) };
        let used = n as f32 * (bw + gap) - gap;
        let x0 = (w - used) / 2.0;
        let wf = self.v.waveform;
        for i in 0..n {
            let (lo, hi) = (i * wf.len() / n, ((i + 1) * wf.len() / n).max(i * wf.len() / n + 1).min(wf.len()));
            let peak = wf.get(lo..hi).and_then(|s| s.iter().max()).copied().unwrap_or(0) as f32 / 255.0;
            let bar = 4.0 + (h - 4.0) * peak.sqrt();
            let c = if (i as f32 + 0.5) / n as f32 <= progress { self.t.primary } else { idle };
            let r = canvas::Path::rounded_rectangle(
                Point::new(x0 + i as f32 * (bw + gap), (h - bar) / 2.0),
                Size::new(bw, bar),
                (bw / 2.0).into(),
            );
            frame.fill(&r, c);
        }
        vec![frame.into_geometry()]
    }
}

/// The inside of a voice bubble: play/pause, waveform filling with progress (click to seek), time.
/// Put it in the chat bubble container. `on_seek` gets the clicked fraction 0..1.
pub fn bubble<'a, M: Clone + 'a>(
    t: Tok,
    v: VoiceView<'a>,
    outgoing: bool,
    on_toggle: M,
    on_seek: impl Fn(f32) -> M + 'a,
) -> Element<'a, M> {
    const PLAY: &str = r#"<path d="M7 4v16l13-8z"/>"#;
    const PAUSE: &str = r#"<path d="M6 4h4v16H6z M14 4h4v16h-4z"/>"#;
    let play = button(container(glyph(if v.playing { PAUSE } else { PLAY }, 18.0, t.on_primary, true)).center(Length::Fill))
        .width(Length::Fixed(40.0))
        .height(Length::Fixed(40.0))
        .padding(0)
        .on_press(on_toggle)
        .style(ui::button_style(t, Kind::Primary, 20.0));
    let shown = match v.position_ms {
        Some(p) if v.playing || p > 0 => p,
        _ => v.duration_ms,
    };
    let wave = canvas(Wave { t, v, outgoing, on_seek: Box::new(on_seek) }).width(Length::Fill).height(Length::Fixed(28.0));
    row![
        play,
        wave,
        text(clock(shown)).font(ui::MONO).size(12).color(if outgoing { t.on_primary_c } else { t.ink2 }),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .width(Length::Fixed(260.0))
    .into()
}

/// The mic button that starts a recording (a click, not a hold).
pub fn mic_button<'a, M: Clone + 'a>(t: Tok, on_press: M) -> Element<'a, M> {
    button(container(ui::icon(Icon::Mic, 22.0, t.on_primary)).center(Length::Fill))
        .width(Length::Fixed(44.0))
        .height(Length::Fixed(44.0))
        .padding(0)
        .on_press(on_press)
        .style(ui::button_style(t, Kind::Primary, 22.0))
        .into()
}

/// Shown instead of the text field while recording: red dot, timer, "Esc to cancel", bin, Send.
pub fn record_bar<'a, M: Clone + 'a>(t: Tok, elapsed_ms: u32, on_send: M, on_cancel: M) -> Element<'a, M> {
    const SEND: &str = r#"<path d="m22 2-7 20-4-9-9-4z"/><path d="M22 2 11 13"/>"#;
    let dot = container(Space::new()).width(Length::Fixed(10.0)).height(Length::Fixed(10.0)).style(ui::plain(t.error, 5.0));
    let bin = button(container(ui::icon(Icon::Trash, 20.0, t.error)).center(Length::Fill))
        .width(Length::Fixed(40.0))
        .height(Length::Fixed(40.0))
        .padding(0)
        .on_press(on_cancel)
        .style(ui::button_style(t, Kind::Ghost, 20.0));
    let send = button(container(glyph(SEND, 20.0, t.on_primary, false)).center(Length::Fill))
        .width(Length::Fixed(44.0))
        .height(Length::Fixed(44.0))
        .padding(0)
        .on_press(on_send)
        .style(ui::button_style(t, Kind::Primary, 22.0));
    container(
        row![
            dot,
            text(clock(elapsed_ms)).font(ui::MONO).size(15).color(t.ink),
            text("Esc to cancel").size(14).color(t.ink2).width(Length::Fill).align_x(Alignment::Center),
            bin,
            send,
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding(iced::Padding { top: 2.0, right: 2.0, bottom: 2.0, left: 16.0 })
    .style(ui::outlined(t.surface, t.line, 24.0))
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downsampler_keeps_rate_and_level() {
        let mut d = Down::new(48_000, 16_000);
        let mut out = Vec::new();
        for i in 0..48_000 {
            d.push((2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48_000.0).sin(), |o| out.push(o));
        }
        assert!((out.len() as i32 - 16_000).abs() <= 1, "{}", out.len());
        let peak = out.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        assert!(peak > 0.9 && peak <= 1.0, "{peak}");
        // Odd ratio (44.1k) stays in step too.
        let mut d = Down::new(44_100, 16_000);
        let mut n = 0i32;
        for _ in 0..44_100 {
            d.push(0.5, |_| n += 1);
        }
        assert!((n - 16_000).abs() <= 1, "{n}");
    }

    #[test]
    fn upsampler_produces_the_ratio() {
        let mut u = Up::new(16_000, 48_000);
        let mut n = 0i32;
        for _ in 0..50 {
            u.process(&[0.1; 320], |_| n += 1);
        }
        assert!((n - 48_000).abs() <= 3, "{n}");
    }

    #[test]
    fn clock_formats() {
        assert_eq!(clock(7_400), "0:07");
        assert_eq!(clock(14 * 60_000 + 59_600), "15:00");
    }
}
