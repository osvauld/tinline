//! System tray icon (StatusNotifier via libappindicator) with Show / Quit. GTK needs its own
//! thread and main loop.

use std::sync::Mutex;
use std::time::Duration;

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};

#[derive(Debug, Clone, Copy)]
pub enum TrayCmd {
    Show,
    Quit,
}

static STATUS: Mutex<Option<String>> = Mutex::new(None);

/// Shows `s` ("Locked", "Online", ...) as the first menu line and in the tooltip. Callable from any thread.
pub fn set_status(s: &str) {
    let mut g = STATUS.lock().unwrap();
    if g.as_deref() != Some(s) {
        *g = Some(s.to_string());
    }
}

fn icon() -> Icon {
    let n = 32usize;
    let mut px = Vec::with_capacity(n * n * 4);
    for y in 0..n {
        for x in 0..n {
            let (dx, dy) = (x as f32 - 15.5, y as f32 - 15.5);
            let d = (dx * dx + dy * dy).sqrt();
            // A teal disc with a white dot.
            if d < 5.0 {
                px.extend_from_slice(&[255, 255, 255, 255]);
            } else if d < 15.0 {
                px.extend_from_slice(&[32, 160, 150, 255]);
            } else {
                px.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    Icon::from_rgba(px, n as u32, n as u32).expect("icon")
}

/// Starts the tray thread; commands arrive through `send`. Returns false if GTK is unusable.
pub fn spawn(send: impl Fn(TrayCmd) + Send + Sync + 'static) -> bool {
    let (ok_tx, ok_rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("tray".into()).spawn(move || {
        if gtk::init().is_err() {
            let _ = ok_tx.send(false);
            return;
        }
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
            .with_tooltip("Osvauld calls")
            .with_icon(icon())
            .build();
        let tray = match tray {
            Ok(t) => t,
            Err(e) => {
                eprintln!("tray unavailable: {e}");
                let _ = ok_tx.send(false);
                return;
            }
        };
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            if e.id == show_id {
                send(TrayCmd::Show);
            } else if e.id == quit_id {
                send(TrayCmd::Quit);
            }
        }));
        let _ = ok_tx.send(true);
        // GTK objects stay on this thread; poll the shared status text from its main loop.
        let mut shown = String::new();
        gtk::glib::timeout_add_local(Duration::from_millis(500), move || {
            let want = STATUS.lock().unwrap().clone();
            if let Some(w) = want.filter(|w| *w != shown) {
                status.set_text(&w);
                let _ = tray.set_tooltip(Some(format!("Osvauld calls - {w}")));
                shown = w;
            }
            gtk::glib::ControlFlow::Continue
        });
        gtk::main();
    });
    spawned.is_ok() && ok_rx.recv().unwrap_or(false)
}
