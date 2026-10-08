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
//!   p2p-peer --data DIR listen [--answer-after SECS] [--second decline|answer|ignore] [AUDIO] [--secs N]
//!   p2p-peer --data DIR recents
//!   p2p-peer --data DIR call DID|NAME [AUDIO] [--secs N]
//!
//! AUDIO: `--tone HZ` or `--wav in.wav` (48 kHz mono) as the mic; `--record out.wav` saves
//! what was heard. Each second of a call prints a `STATS` line; the end prints `RESULT`.

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use p2pcore::{CallInfo, CallState, LockState, Node, NodeEvents, NodeStatus};

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
    /// What to do with a second call that arrives during a call; `ignore` is the default.
    second: String,
}

fn parse() -> Result<Opts, String> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut take = |name: &str| -> Result<Option<String>, String> {
        let Some(i) = args.iter().position(|a| a == name) else { return Ok(None) };
        let v = args.get(i + 1).cloned().ok_or(format!("{name} needs a value"))?;
        args.drain(i..i + 2);
        Ok(Some(v))
    };
    let second = take("--second")?.unwrap_or_else(|| "ignore".into());
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
    let mut flag = |name: &str| match args.iter().position(|a| a == name) {
        Some(i) => {
            args.remove(i);
            true
        }
        None => false,
    };
    let verbose = flag("-v");
    let once = flag("--once");
    Ok(Opts { data, passphrase, tone, wav, record, secs, answer_after, rest: args, verbose, once, second })
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
    match rest.as_slice() {
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
        ["recents"] => {
            for r in node.recent_calls(50) {
                println!("CALL id={} peer={} incoming={} reason={} missed={}", r.call_id, r.peer_name, r.incoming, r.reason, r.missed);
            }
        }
        ["listen"] => {
            start_online(&node)?;
            eprintln!("LISTENING {}", node.status().endpoint_id);
            loop {
                match rx.recv().map_err(|e| e.to_string())? {
                    Event::Incoming(call) => {
                        std::thread::sleep(Duration::from_secs_f64(o.answer_after));
                        node.answer(call.call_id.clone()).map_err(|e| e.to_string())?;
                        let mut id = call.call_id.clone();
                        // `--second answer` swaps to the second call, which then gets its own run.
                        while let Some(next) = converse(&node, &o, &rx, &id)? {
                            id = next;
                        }
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
fn converse(node: &Node, o: &Opts, rx: &mpsc::Receiver<Event>, call_id: &str) -> Result<Option<String>, String> {
    let mut swapped = None;
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
            match ev {
                Event::State(id, CallState::Ended { reason }) if id == call_id => ended = Some(reason),
                Event::State(id, CallState::Ended { reason }) => println!("OTHER_ENDED id={id} reason={reason}"),
                Event::Incoming(c) if c.call_id != call_id => {
                    println!("SECOND_CALL id={} waiting={}", c.call_id, node.waiting_call().is_some_and(|w| w.call_id == c.call_id));
                    match o.second.as_str() {
                        "decline" => {
                            let _ = node.decline(c.call_id);
                        }
                        "answer" => {
                            if node.end_and_answer(c.call_id.clone()).is_ok() {
                                swapped = Some(c.call_id);
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
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
    Ok(swapped)
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
