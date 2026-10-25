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
mod geom;
mod shaped;
mod tallest;
/// Contact blocking on the core's API: state the screens read, and the dialog copy.
mod blocks;
mod audio;
mod cues;
mod flow;
mod hostcond;
mod keystore;
mod hangup;
mod mark;
/// Motion tokens (playground lib/fx/motion.ts) and tweens.
mod motion;
/// The playground's own icon set (icons.ts).
mod pg_icons;
/// Tinline's line icons, generated from the mark's geometry (crates/tin-icons); the source of truth for glyphs.
#[path = "../../tin-icons/generated/tin_icons.rs"]
#[allow(dead_code)]
mod tin_icons;
/// Receipt-tick geometry and spring curves, generated from the string model (crates/tin-icons/src/ticks.rs).
#[path = "../../tin-icons/generated/tin_ticks.rs"]
#[allow(dead_code)]
mod tin_ticks;
/// Receipt ticks drawn as the mark's string, with their one-shot transitions.
mod ticks;
/// CSS-like text ellipsis by measurement.
mod fit;
mod call_field;
mod call_ui;
mod raise;
mod media;
mod camera;
mod frames;
mod video_surface;
mod reason;
mod single;
mod startup;
mod tray;
mod ui;
mod voice;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use p2pcore::{CallInfo, CallState, Chat, ChatEvents, DeliveryState, GroupCallEvent, GroupCallEvents, LockState, Message, Node, NodeEvents, NodeStatus, VideoInfo};
use tokio::sync::mpsc;

/// Everything the node and the tray report, funnelled into the UI's update loop.
pub enum Ev {
    Status(NodeStatus),
    Contacts,
    /// Account-scoped profile invalidation, never decoded on a core callback thread.
    Profile { epoch: u64, did: String },
    Incoming(CallInfo),
    State(String, CallState),
    Tray(tray::TrayCmd),
    /// Audio device trouble worth telling the user about.
    AudioNotice(String),
    Chat(ChatEv),
    Group(GroupEv),
    /// A notification was clicked.
    Open(app::Target),
    Link(LinkEv),
    Video(VideoInfo),
    /// A group-call event (the audio already followed it; the call UI comes next).
    GroupCall(GroupCallEvent),
    /// Someone used our code: "Maya wants to add you as a contact" (once per person).
    Request(p2pcore::ContactRequest),
    /// They accepted the request we sent: our line to them is open.
    Accepted(p2pcore::Contact),
}

/// Bridges `ContactRequestEvents` into the same channel as `NodeEvents`.
pub struct RequestBridge(pub mpsc::UnboundedSender<Ev>);

impl p2pcore::ContactRequestEvents for RequestBridge {
    fn on_contact_request(&self, request: p2pcore::ContactRequest) {
        tlog!("CONTACT_REQUEST {} {}", request.did, request.name);
        let _ = self.0.send(Ev::Request(request));
    }
    fn on_request_accepted(&self, contact: p2pcore::Contact) {
        tlog!("REQUEST_ACCEPTED {} {}", contact.did, contact.name);
        let _ = self.0.send(Ev::Accepted(contact));
    }
}

/// Bridges `VideoEvents` into the same channel as `NodeEvents`.
pub struct VideoBridge(pub mpsc::UnboundedSender<Ev>);

impl p2pcore::VideoEvents for VideoBridge {
    fn on_video_state(&self, info: VideoInfo) {
        tlog!("VIDEO {} negotiated={} local={} remote={}", info.call_id, info.negotiated, info.local_camera, info.remote_camera);
        let _ = self.0.send(Ev::Video(info));
    }
}

/// What the core's `LinkEvents` report (device linking and own-device sync).
pub enum LinkEv {
    Code(String, String),
    Done(String, String),
    Failed(String),
    Devices,
    History,
    Unlinked(String),
}

/// Bridges `LinkEvents` into the same channel as `NodeEvents`.
pub struct LinkBridge(pub mpsc::UnboundedSender<Ev>);

impl p2pcore::LinkEvents for LinkBridge {
    fn on_link_code(&self, code: String, peer_label: String) {
        tlog!("LINK_CODE {code} {peer_label}");
        let _ = self.0.send(Ev::Link(LinkEv::Code(code, peer_label)));
    }
    fn on_link_done(&self, did: String, name: String) {
        tlog!("LINK_DONE {did} {name}");
        let _ = self.0.send(Ev::Link(LinkEv::Done(did, name)));
    }
    fn on_link_failed(&self, reason: String) {
        tlog!("LINK_FAILED {reason}");
        let _ = self.0.send(Ev::Link(LinkEv::Failed(reason)));
    }
    fn on_devices_changed(&self) {
        let _ = self.0.send(Ev::Link(LinkEv::Devices));
    }
    fn on_history_changed(&self) {
        let _ = self.0.send(Ev::Link(LinkEv::History));
    }
    fn on_unlinked(&self, did: String) {
        tlog!("UNLINKED {did}");
        let _ = self.0.send(Ev::Link(LinkEv::Unlinked(did)));
    }
}

/// What the core's `GroupEvents` report.
pub enum GroupEv {
    Message(p2pcore::GroupMessage),
    Changed(String),
    /// A chat request appeared or changed (G2, U3).
    Request(p2pcore::GroupChatRequest),
    /// A group invitation appeared or changed (G3).
    Invitation(p2pcore::GroupInvitation),
    /// The online list of an open group changed (public group §3).
    Presence(String),
    /// The picture of a post changed state (public group §5): `(group, message)`.
    Image(String, String),
}

/// Bridges `GroupEvents` into the same channel as `NodeEvents`.
pub struct GroupBridge(pub mpsc::UnboundedSender<Ev>);

impl p2pcore::GroupRequestEvents for GroupBridge {
    fn on_chat_request(&self, request: p2pcore::GroupChatRequest) {
        let _ = self.0.send(Ev::Group(GroupEv::Request(request)));
    }
}

impl p2pcore::GroupInvitationEvents for GroupBridge {
    fn on_group_invitation(&self, invitation: p2pcore::GroupInvitation) {
        let _ = self.0.send(Ev::Group(GroupEv::Invitation(invitation)));
    }
}

impl p2pcore::GroupEvents for GroupBridge {
    fn on_group_message(&self, message: p2pcore::GroupMessage) {
        let _ = self.0.send(Ev::Group(GroupEv::Message(message)));
    }
    fn on_group_changed(&self, group_id: String) {
        let _ = self.0.send(Ev::Group(GroupEv::Changed(group_id)));
    }
    /// The online list of an open group changed (public group §3): the header count and its list.
    fn on_group_presence(&self, group_id: String) {
        let _ = self.0.send(Ev::Group(GroupEv::Presence(group_id)));
    }
    /// A post's picture arrived or changed state (public group §5): the open thread re-reads it.
    fn on_group_image(&self, group_id: String, message_id: String) {
        let _ = self.0.send(Ev::Group(GroupEv::Image(group_id, message_id)));
    }
}

/// What the core's `ChatEvents` report, as plain data for the UI loop.
pub enum ChatEv {
    Added(Message),
    Changed(Message),
    Chat(Chat),
    Delivery(String, String, DeliveryState),
    Progress { hash: String, done: u64, total: u64, outgoing: bool },
    Presence(String, bool),
}

/// Bridges `ChatEvents` into the same channel as `NodeEvents`.
pub struct ChatBridge(pub mpsc::UnboundedSender<Ev>);

impl ChatEvents for ChatBridge {
    fn on_message_added(&self, message: Message) {
        let _ = self.0.send(Ev::Chat(ChatEv::Added(message)));
    }
    fn on_message_changed(&self, message: Message) {
        let _ = self.0.send(Ev::Chat(ChatEv::Changed(message)));
    }
    fn on_chat_changed(&self, chat: Chat) {
        let _ = self.0.send(Ev::Chat(ChatEv::Chat(chat)));
    }
    fn on_delivery_changed(&self, peer_did: String, message_id: String, delivery: DeliveryState) {
        let _ = self.0.send(Ev::Chat(ChatEv::Delivery(peer_did, message_id, delivery)));
    }
    fn on_transfer_progress(&self, _peer_did: String, hash: String, done: u64, total: u64, outgoing: bool) {
        let _ = self.0.send(Ev::Chat(ChatEv::Progress { hash, done, total, outgoing }));
    }
    fn on_presence_changed(&self, peer_did: String, online: bool) {
        let _ = self.0.send(Ev::Chat(ChatEv::Presence(peer_did, online)));
    }
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
        self.audio.on_incoming(&call.call_id);
        let _ = self.tx.send(Ev::Incoming(call));
    }
    fn on_call_state(&self, id: String, state: CallState) {
        tlog!("STATE {id} {state:?}");
        self.audio.on_state(&id, &state);
        let _ = self.tx.send(Ev::State(id, state));
    }
    fn on_log(&self, _line: String) {}
}

/// Group-call events: the audio follows them on the core's thread, the UI through the channel.
pub struct GroupCallBridge {
    pub tx: mpsc::UnboundedSender<Ev>,
    pub audio: Arc<audio::Ctl>,
}

impl GroupCallEvents for GroupCallBridge {
    fn on_group_call(&self, e: GroupCallEvent) {
        tlog!("GROUP_CALL {:?} {} {}", e.kind, e.call_id, e.detail);
        self.audio.on_group_call();
        let _ = self.tx.send(Ev::GroupCall(e));
    }
}

pub struct Init {
    pub node: Arc<Node>,
    pub data: PathBuf,
    pub hidden: bool,
    pub auto_answer: Option<f64>,
    /// Test hook: what to do a second later with a call that arrives during a call.
    pub waiting_action: Option<String>,
    pub env_tone: Option<f32>,
    pub tray: bool,
    pub tx: mpsc::UnboundedSender<Ev>,
    /// From P2P_PASSPHRASE; test-only.
    pub test_pass: Option<String>,
    pub audio: Arc<audio::Ctl>,
}

pub static INIT: OnceLock<Init> = OnceLock::new();

/// The data directory this instance runs on.
pub fn data_dir() -> PathBuf {
    INIT.get().map(|i| i.data.clone()).unwrap_or_else(std::env::temp_dir)
}

/// SIGTERM (logout, shutdown, `kill`) quits like the tray's Quit: the node stops and the stores
/// close cleanly instead of being repaired at the next start.
fn quit_on_sigterm(tx: mpsc::UnboundedSender<Ev>) {
    #[cfg(unix)]
    std::thread::Builder::new()
        .name("sigterm".into())
        .spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
            rt.block_on(async {
                use tokio::signal::unix::{signal, SignalKind};
                if let Ok(mut term) = signal(SignalKind::terminate()) {
                    term.recv().await;
                    let _ = tx.send(Ev::Tray(tray::TrayCmd::Quit));
                }
            });
        })
        .ok();
    #[cfg(not(unix))]
    let _ = tx;
}

fn default_data_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "osvauld", "Tinline")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("tinline"))
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
    startup::begin();
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
    media::wipe(&data);

    let (tx, rx) = mpsc::unbounded_channel();
    let _ = EV_RX.set(Mutex::new(Some(rx)));
    let audio = Arc::new(audio::Ctl::default());
    let node = Node::new(data.to_string_lossy().into(), Arc::new(Events { tx: tx.clone(), audio: audio.clone() }))
        .map_err(|e| e.to_string())?;
    startup::mark("node");
    audio.attach(&node);
    node.set_chat_events(Arc::new(ChatBridge(tx.clone())));
    node.set_group_events(Arc::new(GroupBridge(tx.clone())));
    node.set_group_request_events(Arc::new(GroupBridge(tx.clone())));
    node.set_contact_request_events(Arc::new(RequestBridge(tx.clone())));
    node.set_group_invitation_events(Arc::new(GroupBridge(tx.clone())));
    node.set_link_events(Arc::new(LinkBridge(tx.clone())));
    node.set_video_events(Arc::new(VideoBridge(tx.clone())));
    node.set_group_call_events(Arc::new(GroupCallBridge { tx: tx.clone(), audio: audio.clone() }));
    hostcond::spawn(&node);
    // No passphrase: the key kept in the keyring opens it, so launch goes straight to Home.
    keystore::auto_unlock(keystore::system(), &data, &node);
    startup::mark("auto_unlock");
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
            if let Some(v) = node.video_stats() {
                tlog!(
                    "VSTATS sent={} key={} send={}x{} kbps={} enc_ms={:.1} decoded={} errors={} recv={}x{} \
                     reports={} overuse={} starved={} drops={} q_ms={} q_base_ms={} delivered_kbps={} draining={}",
                    v.frames_sent, v.keyframes_sent, v.send_width, v.send_height, v.bitrate_bps / 1000,
                    v.encode_ms, v.frames_decoded, v.decode_errors, v.recv_width, v.recv_height,
                    v.rate_reports, v.rate_overuse, v.rate_starved, v.backlog_drops, v.last_queuing_us / 1000,
                    v.queuing_offset_us / 1000, v.last_delivered_bps / 1000, v.draining
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

    quit_on_sigterm(tx.clone());
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
        waiting_action: test_env("P2P_WAITING_ACTION"),
        env_tone: test_env_f("P2P_TEST_TONE").map(|v| v as f32),
        tray,
        tx: tx.clone(),
        test_pass: test_env("P2P_PASSPHRASE").filter(|p| !p.is_empty()),
        audio,
    });
    startup::mark("run");
    app::run().map_err(|e| e.to_string())
}
