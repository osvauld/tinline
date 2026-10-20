//! Dev-only reproducer for the remote-video flicker: plays raw I420 frames at 30 fps through either the old display path
//! (I420 -> RGBA -> a new `image::Handle` every frame) or the video surface, in a window with a magenta backdrop, so a
//! screen recording shows every frame the picture was missing (magenta) as a flicker.
//!
//!   cargo run --release -p desktop --example video_flicker -- <old|new> <frames dir> <WxH> [count] [seconds]
//!
//! The frames dir holds `f-<W>x<H>-<N>.i420` files, played in name order (raw I420, as `vpxdec --i420 --md5`-style dumps). Window 600x1000.

// The app uses the drawn-size report; this reproducer does not.
#[allow(dead_code)]
#[path = "../src/video_surface.rs"]
mod video_surface;

/// video_surface sizes the picture by the app's window scale; this reproducer draws at 1.0.
mod ui {
    pub fn scale() -> f32 {
        1.0
    }
}

use iced::widget::{container, image, image::Handle};
use iced::{time, Color, ContentFit, Element, Fill, Task};
use std::time::{Duration, Instant};
use video::I420;

const BACKDROP: Color = Color::from_rgb(1.0, 0.0, 1.0);

struct Args {
    old: bool,
    frames: Vec<(u32, u32, Vec<u8>)>,
    seconds: u64,
}

struct App {
    args: Args,
    n: usize,
    old: Option<Handle>,
    new: Option<std::sync::Arc<video_surface::Picture>>,
    started: Instant,
    cost_ms: f32,
}

#[derive(Debug, Clone)]
enum Msg {
    Tick,
}

fn boot() -> (App, Task<Msg>) {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let [mode, dir, size, ..] = &a[..] else { panic!("usage: video_flicker <old|new> <dir> <WxH> [count] [seconds]") };
    let (w, h) = size.split_once('x').expect("WxH");
    let (w, h): (u32, u32) = (w.parse().unwrap(), h.parse().unwrap());
    let count: usize = a.get(3).map_or(120, |c| c.parse().unwrap());
    let seconds = a.get(4).map_or(12, |c| c.parse().unwrap());
    let prefix = format!("f-{w}x{h}-");
    let mut names: Vec<_> = std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()?.file_name().into_string().ok()).filter(|n| n.starts_with(&prefix)).collect();
    names.sort();
    let frames = names
        .iter()
        .take(count)
        .filter_map(|n| std::fs::read(format!("{dir}/{n}")).ok())
        .filter(|d| d.len() == I420::len(w as usize, h as usize))
        .map(|d| (w, h, d))
        .collect::<Vec<_>>();
    assert!(!frames.is_empty(), "no frames for {w}x{h} in {dir}");
    eprintln!("loaded {} frames {w}x{h}, mode {mode}", frames.len());
    let app = App { args: Args { old: mode == "old", frames, seconds }, n: 0, old: None, new: None, started: Instant::now(), cost_ms: 0.0 };
    (app, Task::none())
}

fn update(app: &mut App, _: Msg) -> Task<Msg> {
    if app.started.elapsed() > Duration::from_secs(app.args.seconds) {
        return iced::exit();
    }
    let (w, h, d) = &app.args.frames[app.n % app.args.frames.len()];
    app.n += 1;
    let t = Instant::now();
    if app.args.old {
        // What vcall.rs did: I420 -> RGBA on the UI thread, a new handle each frame.
        let pic = I420 { width: *w as usize, height: *h as usize, data: d.clone() };
        app.old = Some(Handle::from_rgba(*w, *h, video::convert::to_rgba(&pic)));
    } else {
        app.new = video_surface::Picture::new(*w, *h, d.clone()).map(std::sync::Arc::new);
    }
    let ms = t.elapsed().as_secs_f32() * 1e3;
    app.cost_ms = if app.n == 1 { ms } else { 0.9 * app.cost_ms + 0.1 * ms };
    if app.n.is_multiple_of(150) {
        eprintln!("VRENDER shown={} ms={:.2} {w}x{h}", app.n, app.cost_ms);
    }
    Task::none()
}

fn view(app: &App) -> Element<'_, Msg> {
    let main: Element<Msg> = match (&app.old, &app.new) {
        (Some(h), _) => image(h.clone()).content_fit(ContentFit::Contain).width(Fill).height(Fill).into(),
        (_, Some(p)) => video_surface::surface(p.clone()),
        _ => iced::widget::Space::new().into(),
    };
    container(main).width(Fill).height(Fill).style(|_| container::Style { background: Some(BACKDROP.into()), ..Default::default() }).into()
}

fn main() -> iced::Result {
    iced::application(boot, update, view)
        .subscription(|_| time::every(Duration::from_millis(33)).map(|_| Msg::Tick))
        .window_size((600.0, 1000.0))
        .run()
}
