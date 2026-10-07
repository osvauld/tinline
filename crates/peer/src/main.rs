//! Headless desktop peer: the same `p2pcore` node the phone runs, with a file or a tone in
//! place of the microphone and a WAV file in place of the speaker. It is the far end for
//! end-to-end tests against the Android app.
//!
//! `--passphrase PASS` (default: env `P2P_PASSPHRASE`, else "test-passphrase") seals the identity
//! on `init` and unlocks it for every other command; a legacy profile is converted on first use.
//!
//!   p2p-peer --data DIR init NAME
//!   p2p-peer --data DIR ticket
//!   p2p-peer --data DIR add TICKET
//!   p2p-peer --data DIR contacts
//!   p2p-peer --data DIR listen [--answer-after SECS] [AUDIO] [--secs N]
//!   p2p-peer --data DIR call DID|NAME [AUDIO] [--secs N]
//!   p2p-peer --data DIR chat-send NAME TEXT [--wait SECS]
//!   p2p-peer --data DIR chat-list NAME [DAY]
//!   p2p-peer --data DIR file-send NAME PATH [--wait SECS]
//!   p2p-peer --data DIR voice-send NAME OGGFILE [--duration-ms N] [--wait SECS]
//!   p2p-peer --data DIR chat-serve          (commands on stdin, events on stdout; see `serve`)
//!   p2p-peer --data DIR listen [--for SECS]   (also receives chat; prints CHAT lines)
//!
//! AUDIO: `--tone HZ` or `--wav in.wav` (48 kHz mono) as the mic; `--record out.wav` saves
//! what was heard. Each second of a call prints a `STATS` line; the end prints `RESULT`.

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use p2pcore::{CallInfo, CallState, Chat, ChatEvents, DeliveryState, LockState, Message, Node, NodeEvents, NodeStatus};

enum Event {
    Incoming(CallInfo),
    State(String, CallState),
}

struct Printer {
    tx: Mutex<mpsc::Sender<Event>>,
    verbose: bool,
}

impl NodeEvents for Printer {
    fn on_status(&self, s: NodeStatus) {
        if self.verbose {
            eprintln!("status: online={} relay={:?}", s.online, s.relay);
        }
    }
    fn on_contacts_changed(&self) {
        eprintln!("contacts changed");
    }
    fn on_incoming_call(&self, call: CallInfo) {
        eprintln!("INCOMING {} from {} ({})", call.call_id, call.peer_name, call.peer_did);
        let _ = self.tx.lock().unwrap().send(Event::Incoming(call));
    }
    fn on_call_state(&self, id: String, state: CallState) {
        eprintln!("STATE {id} {state:?}");
        let _ = self.tx.lock().unwrap().send(Event::State(id, state));
    }
    fn on_log(&self, line: String) {
        if self.verbose {
            eprintln!("log: {line}");
        }
    }
}

struct ChatPrinter;

fn show(m: &Message) -> String {
    let att = m
        .attachment
        .as_ref()
        .map(|a| format!(" file={} size={} state={:?} kind={:?} hash={}", a.name, a.size, a.state, a.kind, a.hash))
        .unwrap_or_default();
    format!(
        "id={} from={} out={} at={} delivery={:?} deleted={} edited={} text={:?}{att}",
        m.id, m.author_did, m.outgoing, m.at, m.delivery, m.deleted, m.edited_at.is_some(), m.text
    )
}

impl ChatEvents for ChatPrinter {
    fn on_message_added(&self, m: Message) {
        println!("CHAT added {}", show(&m));
    }
    fn on_message_changed(&self, m: Message) {
        println!("CHAT changed {}", show(&m));
    }
    fn on_chat_changed(&self, c: Chat) {
        println!("CHAT chat peer={} unread={} preview={:?}", c.peer_did, c.unread, c.preview);
    }
    fn on_delivery_changed(&self, peer: String, id: String, d: DeliveryState) {
        println!("CHAT delivery peer={peer} id={id} {d:?}");
    }
    fn on_transfer_progress(&self, peer: String, hash: String, done: u64, total: u64, outgoing: bool) {
        if done == total {
            println!("CHAT transfer peer={peer} hash={hash} {done}/{total} outgoing={outgoing}");
        }
    }
}

struct Opts {
    data: PathBuf,
    passphrase: String,
    tone: Option<f32>,
    wav: Option<PathBuf>,
    record: Option<PathBuf>,
    secs: u64,
    answer_after: f64,
    rest: Vec<String>,
    verbose: bool,
    once: bool,
    wait: u64,
    duration_ms: u32,
    listen_for: Option<u64>,
}

fn parse() -> Result<Opts, String> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut take = |name: &str| -> Result<Option<String>, String> {
        let Some(i) = args.iter().position(|a| a == name) else { return Ok(None) };
        let v = args.get(i + 1).cloned().ok_or(format!("{name} needs a value"))?;
        args.drain(i..i + 2);
        Ok(Some(v))
    };
    let data = take("--data")?.ok_or("--data DIR is required")?.into();
    let passphrase = take("--passphrase")?
        .or_else(|| std::env::var("P2P_PASSPHRASE").ok())
        .unwrap_or_else(|| "test-passphrase".into());
    let tone = take("--tone")?.map(|v| v.parse().map_err(|_| "bad --tone")).transpose()?;
    let wav = take("--wav")?.map(PathBuf::from);
    let record = take("--record")?.map(PathBuf::from);
    let secs = take("--secs")?.map(|v| v.parse().map_err(|_| "bad --secs")).transpose()?.unwrap_or(10);
    let answer_after =
        take("--answer-after")?.map(|v| v.parse().map_err(|_| "bad --answer-after")).transpose()?.unwrap_or(1.0);
    let wait = take("--wait")?.map(|v| v.parse().map_err(|_| "bad --wait")).transpose()?.unwrap_or(20);
    let duration_ms = take("--duration-ms")?.map(|v| v.parse().map_err(|_| "bad --duration-ms")).transpose()?.unwrap_or(1000);
    let listen_for = take("--for")?.map(|v| v.parse().map_err(|_| "bad --for")).transpose()?;
    let mut flag = |name: &str| match args.iter().position(|a| a == name) {
        Some(i) => {
            args.remove(i);
            true
        }
        None => false,
    };
    let verbose = flag("-v");
    let once = flag("--once");
    Ok(Opts { data, passphrase, tone, wav, record, secs, answer_after, rest: args, verbose, once, wait, duration_ms, listen_for })
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let o = parse()?;
    let (tx, rx) = mpsc::channel();
    let events = Arc::new(Printer { tx: Mutex::new(tx), verbose: o.verbose });
    let node = Node::new(o.data.to_string_lossy().into(), events).map_err(|e| e.to_string())?;
    let rest: Vec<&str> = o.rest.iter().map(String::as_str).collect();
    if rest.first() != Some(&"init") {
        match node.lock_state() {
            LockState::Locked => node.unlock(o.passphrase.clone()).map_err(|e| e.to_string())?,
            LockState::NeedsPassphrase => {
                node.set_passphrase(None, o.passphrase.clone()).map_err(|e| e.to_string())?
            }
            LockState::NoIdentity | LockState::Unlocked => {}
        }
    }
    node.set_chat_events(Arc::new(ChatPrinter));
    match rest.as_slice() {
        ["chat-send", who, text] => {
            start_online(&node)?;
            let did = find(&node, who)?;
            let m = node.send_text(did.clone(), text.to_string(), None).map_err(|e| e.to_string())?;
            println!("SENT {}", m.id);
            finish_send(&node, &did, &m.id, o.wait);
            node.stop();
        }
        ["file-send", who, path] => {
            start_online(&node)?;
            let did = find(&node, who)?;
            let m = node.send_file(did.clone(), path.to_string(), "application/octet-stream".into(), None).map_err(|e| e.to_string())?;
            println!("SENT {}", m.id);
            finish_send(&node, &did, &m.id, o.wait);
            node.stop();
        }
        ["voice-send", who, path] => {
            start_online(&node)?;
            let did = find(&node, who)?;
            let wave: Vec<u8> = (0..64u32).map(|i| (i * 4) as u8).collect();
            let m = node.send_voice(did.clone(), path.to_string(), o.duration_ms, wave).map_err(|e| e.to_string())?;
            println!("SENT {}", m.id);
            finish_send(&node, &did, &m.id, o.wait);
            node.stop();
        }
        ["chat-list", who] => list_day(&node, who, None)?,
        ["chat-list", who, day] => list_day(&node, who, Some(day.to_string()))?,
        ["chat-serve"] => serve(&node)?,
        ["blob-fetch", who, hash, size] => {
            start_online(&node)?;
            let did = find(&node, who)?;
            let n = node.raw_fetch_blob(did, hash.to_string(), size.parse().map_err(|_| "bad size")?).map_err(|e| e.to_string())?;
            println!("RESULT fetched=true bytes={n}");
            node.stop();
        }
        ["init", name] => {
            let phrase = node.create_identity(name.to_string(), o.passphrase.clone()).map_err(|e| e.to_string())?;
            let p = node.profile().unwrap();
            println!("did {}\ndevice {}\nphrase {phrase}", p.did, p.device);
        }
        ["ticket"] => {
            start_online(&node)?;
            println!("{}", node.my_ticket().map_err(|e| e.to_string())?);
            node.stop();
        }
        ["add", ticket] => {
            start_online(&node)?;
            let c = node.add_contact(ticket.to_string()).map_err(|e| e.to_string())?;
            println!("added {} {}", c.name, c.did);
            node.stop();
        }
        ["contacts"] => {
            for c in node.contacts() {
                println!("{}\t{}\t{}", c.name, c.did, c.device);
            }
        }
        ["whoami"] => {
            let p = node.profile().ok_or("no identity; run init")?;
            println!("did {}\nname {}\ndevice {}", p.did, p.name, p.device);
        }
        ["listen"] => {
            start_online(&node)?;
            eprintln!("LISTENING {}", node.status().endpoint_id);
            let deadline = o.listen_for.map(|s| Instant::now() + Duration::from_secs(s));
            loop {
                let ev = match deadline {
                    Some(d) => match rx.recv_timeout(d.saturating_duration_since(Instant::now())) {
                        Ok(e) => e,
                        Err(_) => break,
                    },
                    None => rx.recv().map_err(|e| e.to_string())?,
                };
                match ev {
                    Event::Incoming(call) => {
                        std::thread::sleep(Duration::from_secs_f64(o.answer_after));
                        node.answer(call.call_id.clone()).map_err(|e| e.to_string())?;
                        converse(&node, &o, &rx, &call.call_id)?;
                        if o.once {
                            break;
                        }
                    }
                    Event::State(..) => {}
                }
            }
            node.stop();
        }
        ["call", who] => {
            start_online(&node)?;
            let contact = node
                .contacts()
                .into_iter()
                .find(|c| c.did == *who || c.name == *who)
                .ok_or(format!("no contact {who}"))?;
            let call = node.call(contact.did).map_err(|e| e.to_string())?;
            // Wait for an answer before the audio clock starts.
            loop {
                match rx.recv_timeout(Duration::from_secs(90)).map_err(|_| "no answer within 90 s")? {
                    Event::State(id, CallState::Active) if id == call.call_id => break,
                    Event::State(id, CallState::Ended { reason }) if id == call.call_id => {
                        println!("RESULT ended_before_answer reason={reason:?}");
                        node.stop();
                        return Err(format!("call ended: {reason}"));
                    }
                    _ => {}
                }
            }
            converse(&node, &o, &rx, &call.call_id)?;
            node.stop();
        }
        _ => return Err("see the usage at the top of crates/peer/src/main.rs".into()),
    }
    Ok(())
}

fn start_online(node: &Node) -> Result<(), String> {
    node.start().map_err(|e| e.to_string())?;
    let t = Instant::now();
    while !node.status().online && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
    }
    if !node.status().online {
        eprintln!("warning: no relay yet; only direct paths will work");
    }
    Ok(())
}

/// Runs the 20 ms audio clock for one active call until it ends or `--secs` pass, then hangs
/// up and prints a summary.
fn converse(node: &Node, o: &Opts, rx: &mpsc::Receiver<Event>, call_id: &str) -> Result<(), String> {
    node.set_test_tone(o.tone);
    let mic: Vec<i16> = match &o.wav {
        Some(p) => read_wav(p)?,
        None => Vec::new(),
    };
    let mut heard: Vec<i16> = Vec::new();
    let frame = audio::FRAME;
    let start = Instant::now();
    let mut next = start;
    let mut tick = 0usize;
    let mut ended = None;
    let mut last = None;
    while start.elapsed() < Duration::from_secs(o.secs) {
        // The mic: wav samples (silence once exhausted) or zeros the core replaces with a tone.
        let from = (tick * frame).min(mic.len());
        let to = ((tick + 1) * frame).min(mic.len());
        let mut pcm = mic[from..to].to_vec();
        pcm.resize(frame, 0);
        node.push_mic(pcm);
        heard.extend(node.pull_speaker());
        tick += 1;
        if tick % 50 == 0
            && let Some(s) = node.call_stats()
        {
            println!(
                "STATS t={} direct={} rtt={}ms sent={} recv={} lost={} recovered={} concealed={} buf={}ms freq={:.1} rms={:.0}",
                s.secs, s.direct, s.rtt_ms, s.sent, s.received, s.lost, s.recovered, s.concealed,
                s.buffered_ms, s.rx_freq_hz, s.rx_rms
            );
            last = Some(s);
        }
        while let Ok(ev) = rx.try_recv() {
            if let Event::State(id, CallState::Ended { reason }) = ev
                && id == call_id
            {
                ended = Some(reason);
            }
        }
        if ended.is_some() {
            break;
        }
        next += Duration::from_millis(20);
        if let Some(wait) = next.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    }
    if ended.is_none() {
        let _ = node.hangup(call_id.to_string());
        // Let the hangup reach the peer.
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(3) {
            if let Ok(Event::State(id, CallState::Ended { .. })) = rx.recv_timeout(Duration::from_millis(200))
                && id == call_id
            {
                break;
            }
        }
    }
    node.set_test_tone(None);
    // Skip the first second: jitter-buffer warm-up.
    let settled = &heard[heard.len().min(audio::RATE as usize)..];
    let freq = audio::estimate_frequency(settled).unwrap_or(0.0);
    let rms = audio::rms(settled);
    if let Some(p) = &o.record {
        write_wav(p, &heard)?;
    }
    let s = last.or_else(|| node.call_stats());
    println!(
        "RESULT secs={:.1} heard_samples={} freq={freq:.1} rms={rms:.0} ended_by={} direct={} lost={} recovered={} concealed={}",
        start.elapsed().as_secs_f32(),
        heard.len(),
        ended.as_deref().unwrap_or("us"),
        s.as_ref().map(|s| s.direct).unwrap_or(false),
        s.as_ref().map(|s| s.lost).unwrap_or(0),
        s.as_ref().map(|s| s.recovered).unwrap_or(0),
        s.as_ref().map(|s| s.concealed).unwrap_or(0),
    );
    Ok(())
}

fn read_wav(p: &PathBuf) -> Result<Vec<i16>, String> {
    let mut r = hound::WavReader::open(p).map_err(|e| e.to_string())?;
    let spec = r.spec();
    if spec.sample_rate != audio::RATE || spec.channels != 1 || spec.bits_per_sample != 16 {
        return Err(format!("{}: need 48 kHz mono 16-bit", p.display()));
    }
    r.samples::<i16>().collect::<Result<_, _>>().map_err(|e| e.to_string())
}

fn write_wav(p: &PathBuf, pcm: &[i16]) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: audio::RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(p, spec).map_err(|e| e.to_string())?;
    for s in pcm {
        w.write_sample(*s).map_err(|e| e.to_string())?;
    }
    w.finalize().map_err(|e| e.to_string())
}

fn find(node: &Node, who: &str) -> Result<String, String> {
    node.contacts().into_iter().find(|c| c.did == who || c.name == who).map(|c| c.did).ok_or(format!("no contact {who}"))
}

/// Waits (up to `wait` seconds) for the peer to confirm the message, then prints the result.
fn finish_send(node: &Node, did: &str, id: &str, wait: u64) {
    let end = Instant::now() + Duration::from_secs(wait);
    let mut delivered = false;
    loop {
        let page = node.chat_day(did.to_string(), None);
        if let Ok(p) = page
            && p.messages.iter().any(|m| m.id == id && m.delivery == DeliveryState::Delivered)
        {
            delivered = true;
            break;
        }
        if Instant::now() >= end {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    println!("RESULT delivered={delivered}");
}

fn list_day(node: &Node, who: &str, day: Option<String>) -> Result<(), String> {
    let did = find(node, who)?;
    let page = node.chat_day(did, day).map_err(|e| e.to_string())?;
    println!("DAY {} older={:?}", page.day, page.older_day);
    for m in &page.messages {
        println!("MSG {}", show(m));
    }
    println!("END");
    Ok(())
}

/// Line commands on stdin, answers on stdout (`OK ...`, `ERR ...`); chat events interleave as
/// `CHAT ...` lines. For tests that need one long-lived peer.
fn serve(node: &Node) -> Result<(), String> {
    use std::io::BufRead;
    start_online(node)?;
    println!("READY {}", node.status().endpoint_id);
    for line in std::io::stdin().lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        let parts: Vec<&str> = line.splitn(4, ' ').collect();
        let r: Result<String, String> = (|| match parts.as_slice() {
            ["send", who, text @ ..] => {
                let text = text.join(" ");
                let m = node.send_text(find(node, who)?, text, None).map_err(|e| e.to_string())?;
                Ok(format!("SENT {}", m.id))
            }
            ["reply", who, to, text] => {
                let m = node.send_text(find(node, who)?, text.to_string(), Some(to.to_string())).map_err(|e| e.to_string())?;
                Ok(format!("SENT {}", m.id))
            }
            ["edit", who, id, text] => {
                node.edit_message(find(node, who)?, id.to_string(), text.to_string()).map_err(|e| e.to_string())?;
                Ok("EDITED".into())
            }
            ["delete", who, id] => {
                node.delete_message(find(node, who)?, id.to_string()).map_err(|e| e.to_string())?;
                Ok("DELETED".into())
            }
            ["file", who, path] => {
                let m = node.send_file(find(node, who)?, path.to_string(), "application/octet-stream".into(), None).map_err(|e| e.to_string())?;
                Ok(format!("SENT {}", m.id))
            }
            ["voice", who, path, ms] => {
                let wave: Vec<u8> = (0..64u32).map(|i| (i * 4) as u8).collect();
                let m = node.send_voice(find(node, who)?, path.to_string(), ms.parse().map_err(|_| "bad ms")?, wave).map_err(|e| e.to_string())?;
                Ok(format!("SENT {}", m.id))
            }
            ["get", who, id, out] => {
                let did = find(node, who)?;
                node.download_attachment(did.clone(), id.to_string()).map_err(|e| e.to_string())?;
                let end = Instant::now() + Duration::from_secs(120);
                loop {
                    let ready = node.chat_day(did.clone(), None).map_err(|e| e.to_string())?;
                    let mut all = Vec::new();
                    let mut page = Some(ready);
                    while let Some(p) = page {
                        all.extend(p.messages.clone());
                        page = match p.older_day {
                            Some(d) => node.chat_day(did.clone(), Some(d)).ok(),
                            None => None,
                        };
                    }
                    let m = all.iter().find(|m| m.id == *id).ok_or("no such message")?;
                    if m.attachment.as_ref().is_some_and(|a| matches!(a.state, p2pcore::TransferState::Ready)) {
                        break;
                    }
                    if Instant::now() > end {
                        return Err("download timed out".into());
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
                node.save_attachment(did, id.to_string(), out.to_string()).map_err(|e| e.to_string())?;
                Ok(format!("SAVED {out}"))
            }
            ["list", who] => {
                list_day(node, who, None)?;
                Ok("LISTED".into())
            }
            ["list", who, day] => {
                list_day(node, who, Some(day.to_string()))?;
                Ok("LISTED".into())
            }
            ["history", who, before] => {
                let n = node.fetch_older_history(find(node, who)?, before.to_string()).map_err(|e| e.to_string())?;
                Ok(format!("HISTORY {n}"))
            }
            ["chats"] => {
                for c in node.chats().map_err(|e| e.to_string())? {
                    println!("CHATROW {} unread={} preview={:?}", c.peer_did, c.unread, c.preview);
                }
                Ok("LISTED".into())
            }
            ["markread", who] => {
                node.mark_read(find(node, who)?).map_err(|e| e.to_string())?;
                Ok("OK".into())
            }
            ["remove", who] => {
                node.remove_contact(find(node, who)?).map_err(|e| e.to_string())?;
                Ok("REMOVED".into())
            }
            ["lock"] => {
                node.lock();
                Ok("LOCKED".into())
            }
            ["quit"] => Err("quit".into()),
            _ => Err(format!("unknown command {line:?}")),
        })();
        match r {
            Ok(s) => println!("OK {s}"),
            Err(e) if e == "quit" => break,
            Err(e) => println!("ERR {e}"),
        }
    }
    node.stop();
    Ok(())
}
