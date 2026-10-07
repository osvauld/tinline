//! System tray icon with Show / Quit. Linux: StatusNotifier via libappindicator, GTK on its own
//! thread and main loop. Windows: a Win32 message loop on the tray thread. Other systems: no tray.

use std::sync::Mutex;
#[cfg(any(target_os = "linux", windows))]
use std::time::Duration;

#[cfg(any(target_os = "linux", windows))]
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
#[cfg(any(target_os = "linux", windows))]
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

#[derive(Debug, Clone, Copy)]
pub enum TrayCmd {
    Show,
    Quit,
}

static STATUS: Mutex<Option<String>> = Mutex::new(None);
static LOOK: Mutex<(State, bool)> = Mutex::new((State::Unavailable, false));

/// What the tray icon shows: shape carries the state, not only colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Available,
    Unavailable,
    InCall,
}

/// Shows `s` ("Locked", "Available", ...) as the first menu line and in the tooltip. Callable from any thread.
pub fn set_status(s: &str) {
    let mut g = STATUS.lock().unwrap();
    if g.as_deref() != Some(s) {
        *g = Some(s.to_string());
    }
}

pub fn set_state(st: State) {
    LOOK.lock().unwrap().0 = st;
}

/// The panel is dark: use the light glyphs.
pub fn set_dark(dark: bool) {
    LOOK.lock().unwrap().1 = dark;
}

#[cfg(any(target_os = "linux", windows))]
macro_rules! png {
    ($state:literal, $theme:literal, $size:literal) => {
        include_bytes!(concat!("../assets/tray/", $state, "-", $theme, "-", $size, ".png")) as &[u8]
    };
}

/// The shipped sizes are 16, 22, 24 and 32; Linux panels take 22, Windows scales 32 down.
#[cfg(any(target_os = "linux", windows))]
fn tray_png(state: State, dark: bool) -> &'static [u8] {
    macro_rules! pick {
        ($size:literal) => {
            match (state, dark) {
                (State::Available, false) => png!("available", "light", $size),
                (State::Available, true) => png!("available", "dark", $size),
                (State::Unavailable, false) => png!("unavailable", "light", $size),
                (State::Unavailable, true) => png!("unavailable", "dark", $size),
                (State::InCall, false) => png!("incall", "light", $size),
                (State::InCall, true) => png!("incall", "dark", $size),
            }
        };
    }
    if cfg!(windows) { pick!("32") } else { pick!("22") }
}

/// Decodes a PNG to straight RGBA.
fn decode(png_bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(png_bytes)).read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
        _ => return None,
    };
    Some((rgba, info.width, info.height))
}

/// The window / taskbar icon.
pub fn window_icon() -> Option<iced::window::Icon> {
    let (px, w, h) = decode(include_bytes!("../assets/tray/app-128.png"))?;
    iced::window::icon::from_rgba(px, w, h).ok()
}

#[cfg(any(target_os = "linux", windows))]
fn icon(state: State, dark: bool) -> Icon {
    let (px, w, h) = decode(tray_png(state, dark)).expect("tray icon png");
    Icon::from_rgba(px, w, h).expect("icon")
}

/// How long to wait for the tray thread to report; a stuck GTK/shell is treated as "no tray".
#[cfg(any(target_os = "linux", windows))]
const START_TIMEOUT: Duration = Duration::from_secs(5);

/// Builds the menu and icon on the calling (tray) thread and wires the menu events to `send`.
#[cfg(any(target_os = "linux", windows))]
fn build(send: impl Fn(TrayCmd) + Send + Sync + 'static) -> Result<(MenuItem, TrayIcon), String> {
    let menu = Menu::new();
    let status = MenuItem::new("Starting...", false, None);
    let show = MenuItem::new("Show", true, None);
    let quit = MenuItem::new("Quit", true, None);
    let _ = menu.append(&status);
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&show);
    let _ = menu.append(&quit);
    let (show_id, quit_id) = (show.id().clone(), quit.id().clone());
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Tinline")
        .with_icon(icon(State::Unavailable, false))
        .build()
        .map_err(|e| e.to_string())?;
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        if e.id == show_id {
            send(TrayCmd::Show);
        } else if e.id == quit_id {
            send(TrayCmd::Quit);
        }
    }));
    Ok((status, tray))
}

/// Pushes a changed status text into the menu line and tooltip.
#[cfg(any(target_os = "linux", windows))]
fn refresh(status: &MenuItem, tray: &TrayIcon, shown: &mut String, look: &mut Option<(State, bool)>) {
    let want = STATUS.lock().unwrap().clone();
    if let Some(w) = want.filter(|w| w != shown) {
        status.set_text(&w);
        let _ = tray.set_tooltip(Some(format!("Tinline - {w}")));
        *shown = w;
    }
    let l = *LOOK.lock().unwrap();
    if *look != Some(l) {
        let _ = tray.set_icon(Some(icon(l.0, l.1)));
        *look = Some(l);
    }
}

/// Starts the tray thread; commands arrive through `send`. Returns false if the tray is unusable.
#[cfg(any(target_os = "linux", windows))]
pub fn spawn(send: impl Fn(TrayCmd) + Send + Sync + 'static) -> bool {
    let (ok_tx, ok_rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("tray".into()).spawn(move || {
        #[cfg(target_os = "linux")]
        if gtk::init().is_err() {
            let _ = ok_tx.send(false);
            return;
        }
        let (status, tray) = match build(send) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("tray unavailable: {e}");
                let _ = ok_tx.send(false);
                return;
            }
        };
        let _ = ok_tx.send(true);
        let (mut shown, mut look) = (String::new(), None);
        #[cfg(target_os = "linux")]
        {
            // GTK objects stay on this thread; poll the shared status text from its main loop.
            gtk::glib::timeout_add_local(Duration::from_millis(500), move || {
                refresh(&status, &tray, &mut shown, &mut look);
                gtk::glib::ControlFlow::Continue
            });
            gtk::main();
        }
        #[cfg(windows)]
        {
            use std::ptr::null_mut;
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                DispatchMessageW, MsgWaitForMultipleObjects, PeekMessageW, TranslateMessage, MSG,
                PM_REMOVE, QS_ALLINPUT,
            };
            // The tray's hidden window lives on this thread, so its messages must be pumped here.
            loop {
                unsafe {
                    MsgWaitForMultipleObjects(0, std::ptr::null(), 0, 500, QS_ALLINPUT);
                    let mut msg: MSG = std::mem::zeroed();
                    while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
                refresh(&status, &tray, &mut shown, &mut look);
            }
        }
    });
    spawned.is_ok() && ok_rx.recv_timeout(START_TIMEOUT).unwrap_or(false)
}

/// No tray on this platform; the window then minimizes instead of closing.
#[cfg(not(any(target_os = "linux", windows)))]
pub fn spawn(_send: impl Fn(TrayCmd) + Send + Sync + 'static) -> bool {
    false
}
