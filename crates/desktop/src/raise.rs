//! Getting the window in front of the user when the app shows it (tray, notification click, a
//! second launch, a call coming in).
//!
//! What iced 0.14 / winit 0.30 can do on Wayland: `gain_focus` is a no-op there, and
//! `request_user_attention` goes through xdg-activation but only marks the window urgent (no
//! activation token from a user action is passed, and iced exposes neither
//! `request_activation_token` nor a token at window creation). So that part is done with the
//! compositor's own protocol where we know one: under sway the window is moved to the workspace
//! the user is on and focused with `swaymsg`. Elsewhere this does nothing.

/// Asks the compositor to bring our window to the user's current workspace and focus it.
pub fn to_user() {
    #[cfg(target_os = "linux")]
    sway();
}

#[cfg(target_os = "linux")]
fn sway() {
    if std::env::var_os("SWAYSOCK").is_none() {
        return;
    }
    let pid = std::process::id();
    let _ = std::thread::Builder::new().name("raise".into()).spawn(move || {
        // A window opened just now is not mapped yet; keep trying for a while.
        for _ in 0..60 {
            let out = std::process::Command::new("swaymsg")
                .arg(format!("[pid={pid}] move container to workspace current; [pid={pid}] focus"))
                .output();
            match out {
                Ok(o) if o.status.success() && !String::from_utf8_lossy(&o.stdout).contains("\"success\": false") => return,
                Ok(_) => std::thread::sleep(std::time::Duration::from_millis(250)),
                Err(_) => return, // no swaymsg
            }
        }
    });
}
