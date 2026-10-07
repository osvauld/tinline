//! Tinline desktop: the voice-call core with iced UI, cpal audio with WebRTC echo
//! cancellation, system tray, and the node kept alive while the window is closed.
//!
//!   p2p-desktop [--data DIR] [--hidden] [--print-ticket]
//!
//! Test env (only with `--features test-hooks`, never in release builds): P2P_AUTO_ANSWER=<secs>,
//! P2P_TEST_TONE=<hz>, P2P_PASSPHRASE=<pass> (unlocks the vault non-interactively; the normal app
//! always asks for the passphrase and never stores it), P2P_NO_APM, P2P_SCREEN, plus the
//! TICKET/STATS/INCOMING/STATE stderr lines the e2e scripts parse.

/// stderr diagnostics the e2e scripts parse; compiled out of normal builds.
macro_rules! tlog {
    ($($a:tt)*) => {
        #[cfg(feature = "test-hooks")]
        eprintln!($($a)*);
    };
}
pub(crate) use tlog;

mod app;
mod audio;
mod reason;
mod single;
mod tray;
mod ui;
#[allow(dead_code)]
mod voice;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use p2pcore::{CallInfo, CallState, LockState, Node, NodeEvents, NodeStatus};
use tokio::sync::mpsc;

/// Everything the node and the tray report, funnelled into the UI's update loop.
pub enum Ev {
    Status(NodeStatus),
    Contacts,
    Incoming(CallInfo),
    State(String, CallState),
    Tray(tray::TrayCmd),
    /// Audio device trouble worth telling the user about.
    AudioNotice(String),
}

pub type EvRx = mpsc::UnboundedReceiver<Ev>;
pub static EV_RX: OnceLock<Mutex<Option<EvRx>>> = OnceLock::new();

pub struct Events {
    pub tx: mpsc::UnboundedSender<Ev>,
    pub audio: Arc<audio::Ctl>,
}

impl NodeEvents for Events {
    fn on_status(&self, s: NodeStatus) {
        let _ = self.tx.send(Ev::Status(s));
    }
    fn on_contacts_changed(&self) {
        let _ = self.tx.send(Ev::Contacts);
    }
    fn on_incoming_call(&self, call: CallInfo) {
        tlog!("INCOMING {} from {} ({})", call.call_id, call.peer_name, call.peer_did);
        self.audio.on_incoming();
        let _ = self.tx.send(Ev::Incoming(call));
    }
    fn on_call_state(&self, id: String, state: CallState) {
        tlog!("STATE {id} {state:?}");
        self.audio.on_state(&state);
        let _ = self.tx.send(Ev::State(id, state));
    }
    fn on_log(&self, _line: String) {}
}

pub struct Init {
    pub node: Arc<Node>,
    pub data: PathBuf,
    pub hidden: bool,
    pub auto_answer: Option<f64>,
    pub env_tone: Option<f32>,
    pub tray: bool,
    pub tx: mpsc::UnboundedSender<Ev>,
    /// From P2P_PASSPHRASE; test-only.
    pub test_pass: Option<String>,
    pub audio: Arc<audio::Ctl>,
}

pub static INIT: OnceLock<Init> = OnceLock::new();

/// The data directory before the rename to Tinline.
fn legacy_data_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "osvauld-p2p").map(|d| d.data_dir().to_path_buf())
}

fn default_data_dir() -> PathBuf {
    let new = directories::ProjectDirs::from("com", "osvauld", "Tinline")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("tinline"));
    if let Some(old) = legacy_data_dir() {
        match migrate_data_dir(&old, &new) {
            Ok(true) => eprintln!("Tinline: moved {} to {}", old.display(), new.display()),
            Ok(false) => {}
            Err(e) => {
                // Never run on a fresh empty directory while an identity sits in the old one:
                // that would look like data loss. Keep using the old location instead.
                eprintln!("Tinline: could not move {} to {}: {e}; using the old location", old.display(), new.display());
                return old;
            }
        }
    }
    new
}

/// First start after the rename: moves the old data directory to the new one. Only when the old
/// one has an identity and the new one has none, so an existing install never loses its keys and a
/// new install never touches anything. Returns whether it moved.
fn migrate_data_dir(old: &std::path::Path, new: &std::path::Path) -> std::io::Result<bool> {
    if old == new || !old.join("profile.json").exists() || new.join("profile.json").exists() {
        return Ok(false);
    }
    // An empty directory the new version created already is fine to replace; anything else is not.
    if new.exists() && std::fs::read_dir(new)?.next().is_some() {
        return Err(std::io::Error::other("the new directory is not empty"));
    }
    if let Some(parent) = new.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if new.exists() {
        std::fs::remove_dir(new)?;
    }
    if std::fs::rename(old, new).is_ok() {
        return Ok(true);
    }
    // Different filesystems: copy everything, and only then drop the old copy.
    copy_dir(old, new).inspect_err(|_| {
        let _ = std::fs::remove_dir_all(new);
    })?;
    std::fs::remove_dir_all(old)?;
    Ok(true)
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let target = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &target)?;
        } else {
            std::fs::copy(e.path(), &target)?;
        }
    }
    Ok(())
}

/// Lets scripts/desktop_e2e.py tell a test-hooks binary from a normal one.
#[cfg(feature = "test-hooks")]
#[used]
static TEST_HOOKS_MARKER: &str = "p2p-desktop-test-hooks-enabled";

/// Test-only environment knobs: always None unless built with `--features test-hooks`.
pub fn test_env(k: &str) -> Option<String> {
    if cfg!(feature = "test-hooks") { std::env::var(k).ok() } else { None }
}

fn test_env_f(k: &str) -> Option<f64> {
    test_env(k).and_then(|v| v.parse().ok())
}

fn main() -> Result<(), String> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut data = None;
    if let Some(i) = args.iter().position(|a| a == "--data") {
        data = Some(PathBuf::from(args.get(i + 1).ok_or("--data needs a value")?));
        args.drain(i..i + 2);
    }
    let mut flag = |name: &str| match args.iter().position(|a| a == name) {
        Some(i) => {
            args.remove(i);
            true
        }
        None => false,
    };
    let hidden = flag("--hidden");
    let print_ticket = flag("--print-ticket");
    if let Some(a) = args.first() {
        return Err(format!("unknown argument {a}\nusage: p2p-desktop [--data DIR] [--hidden] [--print-ticket]"));
    }
    let data = data.unwrap_or_else(default_data_dir);
    std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
    // One instance per data dir: two nodes on the same keys and state files would corrupt them.
    // The OS drops the lock when the process dies, so a crash never leaves a stale one.
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(data.join("instance.lock"))
        .map_err(|e| format!("instance.lock: {e}"))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            // Ask the running instance to show its window, then leave.
            if !single::poke(&data) {
                eprintln!("Tinline is already running for {} (look for its tray icon).", data.display());
            }
            return Ok(());
        }
        Err(std::fs::TryLockError::Error(e)) => return Err(format!("instance.lock: {e}")),
    }
    std::mem::forget(lock); // held until the process exits

    let (tx, rx) = mpsc::unbounded_channel();
    let _ = EV_RX.set(Mutex::new(Some(rx)));
    let audio = Arc::new(audio::Ctl::default());
    let node = Node::new(data.to_string_lossy().into(), Arc::new(Events { tx: tx.clone(), audio: audio.clone() }))
        .map_err(|e| e.to_string())?;
    audio.attach(&node);
    let notice_tx = tx.clone();
    audio.set_notice(move |m| {
        let _ = notice_tx.send(Ev::AudioNotice(m));
    });

    // The e2e scripts read call stats from stderr; sampled here, not in the UI loop, which a
    // window the compositor is not drawing can stall.
    #[cfg(feature = "test-hooks")]
    {
        let node = node.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(1));
            if let Some(st) = node.call_stats().filter(|st| st.state == CallState::Active) {
                tlog!(
                    "STATS t={} direct={} rtt={}ms sent={} recv={} lost={} recovered={} concealed={} buf={}ms freq={:.1} rms={:.0}",
                    st.secs, st.direct, st.rtt_ms, st.sent, st.received, st.lost,
                    st.recovered, st.concealed, st.buffered_ms, st.rx_freq_hz, st.rx_rms
                );
            }
        });
    }

    if print_ticket {
        if !node.has_identity() {
            return Err("no identity yet; create one in the app first".into());
        }
        if node.lock_state() == LockState::Locked {
            #[cfg(feature = "test-hooks")]
            {
                let pass = std::env::var("P2P_PASSPHRASE").map_err(|_| "identity is locked; set P2P_PASSPHRASE")?;
                node.unlock(pass).map_err(|e| e.to_string())?;
            }
            #[cfg(not(feature = "test-hooks"))]
            return Err("identity is locked; unlock it in the app (--print-ticket needs an unlocked identity)".into());
        }
        node.start().map_err(|e| e.to_string())?;
        let t = Instant::now();
        while !node.status().online && t.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(100));
        }
        println!("{}", node.my_ticket().map_err(|e| e.to_string())?);
        node.stop();
        return Ok(());
    }

    let show_tx = tx.clone();
    single::listen(&data, move || {
        let _ = show_tx.send(Ev::Tray(tray::TrayCmd::Show));
    });
    let tray_tx = tx.clone();
    let tray = tray::spawn(move |c| {
        let _ = tray_tx.send(Ev::Tray(c));
    });
    if !tray {
        eprintln!("tray: unavailable, closing the window will keep the app running without an icon");
    }
    let _ = INIT.set(Init {
        node,
        data,
        hidden: hidden && tray,
        auto_answer: test_env_f("P2P_AUTO_ANSWER"),
        env_tone: test_env_f("P2P_TEST_TONE").map(|v| v as f32),
        tray,
        tx: tx.clone(),
        test_pass: test_env("P2P_PASSPHRASE").filter(|p| !p.is_empty()),
        audio,
    });
    app::run().map_err(|e| e.to_string())
}

#[cfg(test)]
mod migrate_tests;
