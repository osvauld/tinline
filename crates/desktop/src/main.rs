//! Desktop app for the P2P voice-call core: iced UI, cpal audio with WebRTC echo
//! cancellation, system tray, and the node kept alive while the window is closed.
//!
//!   p2p-desktop [--data DIR] [--hidden] [--print-ticket]
//!
//! Test env: P2P_AUTO_ANSWER=<secs>, P2P_TEST_TONE=<hz>.

mod app;
mod audio;
mod tray;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use p2pcore::{CallInfo, CallState, Node, NodeEvents, NodeStatus};
use tokio::sync::mpsc;

/// Everything the node and the tray report, funnelled into the UI's update loop.
pub enum Ev {
    Status(NodeStatus),
    Contacts,
    Incoming(CallInfo),
    State(String, CallState),
    Tray(tray::TrayCmd),
}

pub type EvRx = mpsc::UnboundedReceiver<Ev>;
pub static EV_RX: OnceLock<Mutex<Option<EvRx>>> = OnceLock::new();

struct Events {
    tx: mpsc::UnboundedSender<Ev>,
}

impl NodeEvents for Events {
    fn on_status(&self, s: NodeStatus) {
        let _ = self.tx.send(Ev::Status(s));
    }
    fn on_contacts_changed(&self) {
        let _ = self.tx.send(Ev::Contacts);
    }
    fn on_incoming_call(&self, call: CallInfo) {
        eprintln!("INCOMING {} from {} ({})", call.call_id, call.peer_name, call.peer_did);
        let _ = self.tx.send(Ev::Incoming(call));
    }
    fn on_call_state(&self, id: String, state: CallState) {
        eprintln!("STATE {id} {state:?}");
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
}

pub static INIT: OnceLock<Init> = OnceLock::new();

fn default_data_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "osvauld-p2p")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("osvauld-p2p"))
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
    let env_f = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<f64>().ok());

    let (tx, rx) = mpsc::unbounded_channel();
    let _ = EV_RX.set(Mutex::new(Some(rx)));
    let node = Node::new(data.to_string_lossy().into(), Arc::new(Events { tx: tx.clone() }))
        .map_err(|e| e.to_string())?;

    if print_ticket {
        if !node.has_identity() {
            return Err("no identity yet; create one in the app first".into());
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
        auto_answer: env_f("P2P_AUTO_ANSWER"),
        env_tone: env_f("P2P_TEST_TONE").map(|v| v as f32),
        tray,
    });
    app::run().map_err(|e| e.to_string())
}
