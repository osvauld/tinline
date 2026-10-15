//! Audio devices for calls: cpal capture/playback at 48 kHz mono as far as the hardware
//! allows (anything else is converted), WebRTC audio processing (echo cancellation, noise
//! suppression, high-pass) on the mic with the render reference fed from the playback side,
//! 20 ms framing to and from the core, and a ring tone.
//!
//! The device callbacks never allocate or lock: they only convert samples and move them through
//! preallocated single-producer single-consumer rings. A worker thread per session does the core
//! push/pull, echo cancellation and framing, and rebuilds a stream whose device went away.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, ErrorKind, FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig};
use p2pcore::Node;
use rtrb::{Consumer, Producer, RingBuffer};
use webrtc_audio_processing::config::{Config, EchoCanceller, HighPassFilter, NoiseSuppression};
use webrtc_audio_processing::Processor;

const RATE: u32 = audio::RATE;
const FRAME: usize = audio::FRAME;
/// WebRTC audio processing works on 10 ms frames.
const APM_FRAME: usize = 480;
/// Mic ring: how much captured audio may wait for the worker (500 ms).
const MIC_RING: usize = RATE as usize / 2;
/// Speaker ring size (200 ms) and how full the worker keeps it (30 ms). The worker only pulls
/// from the core when the device has drained the ring, so playback stays locked to the device clock.
const SPK_RING: usize = RATE as usize / 5;
const SPK_TARGET: usize = RATE as usize * 30 / 1000;
/// What the echo canceller is told about the render-to-capture path: ring fill plus device buffers.
const AEC_DELAY_HINT_MS: u16 = 60;
const WORKER_TICK: Duration = Duration::from_millis(5);
const REOPEN_EVERY: Duration = Duration::from_secs(2);

/// Reports something the user should know about (a device went away) to the UI.
pub type Notice = Arc<dyn Fn(String) + Send + Sync>;

/// Names of the input and output devices the host offers.
pub fn list_devices() -> (Vec<String>, Vec<String>) {
    let host = cpal::default_host();
    let names = |it: Option<Vec<Device>>| {
        let mut v: Vec<String> = it.unwrap_or_default().iter().map(|d| d.to_string()).collect();
        v.dedup();
        v
    };
    let ins = host.input_devices().ok().map(|d| d.collect());
    let outs = host.output_devices().ok().map(|d| d.collect());
    (names(ins), names(outs))
}

fn find(name: &Option<String>, input: bool) -> Option<Device> {
    let host = cpal::default_host();
    if let Some(n) = name {
        let mut devs: Vec<Device> = if input {
            host.input_devices().ok()?.collect()
        } else {
            host.output_devices().ok()?.collect()
        };
        if let Some(i) = devs.iter().position(|d| d.to_string() == *n) {
            return Some(devs.swap_remove(i));
        }
        eprintln!("audio: device {n:?} not found, using the default");
    }
    if input { host.default_input_device() } else { host.default_output_device() }
}

/// Lower is better. Formats cpal can hand us that convert cleanly to f32.
fn format_rank(f: SampleFormat) -> Option<u8> {
    Some(match f {
        SampleFormat::F32 => 0,
        SampleFormat::I16 => 1,
        SampleFormat::I32 => 2,
        SampleFormat::I24 => 3,
        SampleFormat::F64 => 4,
        SampleFormat::U16 => 5,
        SampleFormat::U8 | SampleFormat::I8 | SampleFormat::U24 | SampleFormat::U32 => 6,
        _ => return None,
    })
}

/// Prefers 48 kHz with the fewest channels and the best format; otherwise the device default
/// (the stream is then resampled).
fn pick_config(dev: &Device, input: bool) -> Result<(StreamConfig, SampleFormat), String> {
    let ranges: Vec<_> = if input {
        dev.supported_input_configs().map_err(|e| e.to_string())?.collect()
    } else {
        dev.supported_output_configs().map_err(|e| e.to_string())?.collect()
    };
    let best = ranges
        .into_iter()
        .filter(|r| r.contains_rate(RATE))
        .filter_map(|r| format_rank(r.sample_format()).map(|k| (r.channels(), k, r)))
        .min_by_key(|(ch, k, _)| (*ch, *k));
    if let Some((_, _, r)) = best {
        let c = r.with_sample_rate(RATE);
        return Ok((c.config(), c.sample_format()));
    }
    let c = if input { dev.default_input_config() } else { dev.default_output_config() }
        .map_err(|e| e.to_string())?;
    if format_rank(c.sample_format()).is_none() {
        return Err(format!("unsupported sample format {}", c.sample_format()));
    }
    Ok((c.config(), c.sample_format()))
}

/// Streaming linear resampler for pushed input; allocation-free.
struct Lin {
    step: f64,
    pos: f64,
    last: f32,
}

impl Lin {
    fn new(from: u32, to: u32) -> Self {
        Self { step: from as f64 / to as f64, pos: 0.0, last: 0.0 }
    }

    fn process(&mut self, input: &[f32], mut emit: impl FnMut(f32)) {
        if input.is_empty() {
            return;
        }
        // The virtual buffer is [last, input...].
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

/// Error callback shared by both directions; only errors that mean the stream is dead flag it
/// for a rebuild.
fn on_stream_error(failed: Arc<AtomicBool>, what: &'static str) -> impl FnMut(cpal::Error) + Send + 'static {
    move |e| {
        eprintln!("{what}: {e}");
        if matches!(
            e.kind(),
            ErrorKind::DeviceNotAvailable | ErrorKind::StreamInvalidated | ErrorKind::HostUnavailable
        ) {
            failed.store(true, Ordering::Relaxed);
        }
    }
}

fn build_in<T>(
    dev: &Device,
    cfg: StreamConfig,
    mut prod: Producer<f32>,
    failed: Arc<AtomicBool>,
) -> Result<Stream, String>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let ch = (cfg.channels as usize).max(1);
    let mut lin = (cfg.sample_rate != RATE).then(|| Lin::new(cfg.sample_rate, RATE));
    let mut mono: Vec<f32> = Vec::with_capacity(16384);
    dev.build_input_stream(
        cfg,
        move |d: &[T], _: &_| {
            mono.clear();
            for fr in d.chunks_exact(ch) {
                let sum: f32 = fr.iter().map(|s| f32::from_sample(*s)).sum();
                mono.push(sum / ch as f32);
            }
            match &mut lin {
                Some(l) => l.process(&mono, |s| { let _ = prod.push(s); }),
                None => mono.iter().for_each(|s| { let _ = prod.push(*s); }),
            }
        },
        on_stream_error(failed, "mic"),
        None,
    )
    .map_err(|e| e.to_string())
}

fn build_out<T>(
    dev: &Device,
    cfg: StreamConfig,
    mut cons: Consumer<f32>,
    failed: Arc<AtomicBool>,
) -> Result<Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let ch = (cfg.channels as usize).max(1);
    let step = RATE as f64 / cfg.sample_rate as f64;
    // Linear interpolation between two ring samples; underruns play silence.
    let (mut prev, mut next, mut pos) = (0.0f32, 0.0f32, 1.0f64);
    dev.build_output_stream(
        cfg,
        move |d: &mut [T], _: &_| {
            // chunks_mut also covers a trailing partial frame.
            for fr in d.chunks_mut(ch) {
                while pos >= 1.0 {
                    prev = next;
                    next = cons.pop().unwrap_or(0.0);
                    pos -= 1.0;
                }
                let v = (prev + (next - prev) * pos as f32).clamp(-1.0, 1.0);
                pos += step;
                fr.fill(T::from_sample(v));
            }
        },
        on_stream_error(failed, "speaker"),
        None,
    )
    .map_err(|e| e.to_string())
}

/// Calls `$f::<T>(args)` with the sample type for runtime format `$fmt`.
macro_rules! by_format {
    ($fmt:expr, $f:ident($($arg:expr),*)) => {
        match $fmt {
            SampleFormat::F32 => $f::<f32>($($arg),*),
            SampleFormat::F64 => $f::<f64>($($arg),*),
            SampleFormat::I8 => $f::<i8>($($arg),*),
            SampleFormat::I16 => $f::<i16>($($arg),*),
            SampleFormat::I24 => $f::<cpal::I24>($($arg),*),
            SampleFormat::I32 => $f::<i32>($($arg),*),
            SampleFormat::U8 => $f::<u8>($($arg),*),
            SampleFormat::U16 => $f::<u16>($($arg),*),
            SampleFormat::U24 => $f::<cpal::U24>($($arg),*),
            SampleFormat::U32 => $f::<u32>($($arg),*),
            other => Err(format!("unsupported sample format {other}")),
        }
    };
}

/// An open microphone: mono 48 kHz samples arrive in `cons`.
struct Mic {
    _stream: Stream,
    cons: Consumer<f32>,
    failed: Arc<AtomicBool>,
}

fn open_mic(name: &Option<String>) -> Result<Mic, String> {
    let dev = find(name, true).ok_or("no input device")?;
    let (cfg, fmt) = pick_config(&dev, true)?;
    let (prod, cons) = RingBuffer::new(MIC_RING);
    let failed = Arc::new(AtomicBool::new(false));
    let stream = by_format!(fmt, build_in(&dev, cfg, prod, failed.clone()))?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(Mic { _stream: stream, cons, failed })
}

/// An open speaker: mono 48 kHz samples go into `prod`.
struct Spk {
    _stream: Stream,
    prod: Producer<f32>,
    failed: Arc<AtomicBool>,
}

impl Spk {
    fn queued(&self) -> usize {
        SPK_RING - self.prod.slots()
    }
}

fn open_spk(name: &Option<String>) -> Result<Spk, String> {
    let dev = find(name, false).ok_or("no output device")?;
    let (cfg, fmt) = pick_config(&dev, false)?;
    let (prod, cons) = RingBuffer::new(SPK_RING);
    let failed = Arc::new(AtomicBool::new(false));
    let stream = by_format!(fmt, build_out(&dev, cfg, cons, failed.clone()))?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(Spk { _stream: stream, prod, failed })
}

/// A device that is opened, watched for failure, and reopened (on the system default) when it dies.
struct Slot<D> {
    label: &'static str,
    dev: Option<D>,
    /// Chosen device name; cleared when it fails so the retry follows the system default.
    name: Option<String>,
    next_try: Instant,
    /// A notice about trouble was already sent and not yet followed by a recovery.
    notified: bool,
}

impl<D> Slot<D> {
    fn new(label: &'static str, name: Option<String>) -> Self {
        Self { label, dev: None, name, next_try: Instant::now(), notified: false }
    }

    fn maintain(
        &mut self,
        failed: impl Fn(&D) -> bool,
        open: impl Fn(&Option<String>) -> Result<D, String>,
        notice: &Notice,
    ) {
        if self.dev.as_ref().is_some_and(&failed) {
            self.dev = None;
            self.name = None;
            self.next_try = Instant::now();
            if !self.notified {
                notice(format!("{} disconnected, switching to the system default", self.label));
                self.notified = true;
            }
        }
        if self.dev.is_none() && Instant::now() >= self.next_try {
            match open(&self.name) {
                Ok(d) => {
                    self.dev = Some(d);
                    if self.notified {
                        notice(format!("{} is back", self.label));
                        self.notified = false;
                    }
                }
                Err(e) => {
                    eprintln!("{}: {e}", self.label);
                    if !self.notified {
                        notice(format!("{} unavailable: {e}", self.label));
                        self.notified = true;
                    }
                    self.next_try = Instant::now() + REOPEN_EVERY;
                }
            }
        }
    }
}

/// A running audio session; dropping it (or `stop`) tears the streams down.
pub struct Session {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl Session {
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Mic and speaker for an active call. cpal streams stay on the thread that built them.
pub fn start_call(
    node: Arc<Node>,
    in_dev: Option<String>,
    out_dev: Option<String>,
    muted: Arc<AtomicBool>,
    notice: Notice,
) -> Session {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let join = thread::Builder::new()
        .name("call-audio".into())
        .spawn(move || run_call(node, in_dev, out_dev, muted, flag, notice))
        .ok();
    Session { stop, join }
}

fn run_call(
    node: Arc<Node>,
    in_dev: Option<String>,
    out_dev: Option<String>,
    muted: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    notice: Notice,
) {
    let apm = match Processor::new(RATE) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("audio processing unavailable: {e:?}");
            notice("Echo cancellation is unavailable".into());
            return;
        }
    };
    apm.set_config(Config {
        echo_canceller: Some(EchoCanceller::Full { stream_delay_ms: Some(AEC_DELAY_HINT_MS) }),
        high_pass_filter: Some(HighPassFilter::default()),
        noise_suppression: Some(NoiseSuppression::default()),
        ..Default::default()
    });
    // Test knob: skip the cleaning to check the device path on its own.
    let bypass = crate::test_env("P2P_NO_APM").is_some();

    let mut mic = Slot::<Mic>::new("Microphone", in_dev);
    let mut spk = Slot::<Spk>::new("Speaker", out_dev);
    let mut pcm: Vec<f32> = Vec::with_capacity(RATE as usize);
    let mut frame = vec![0.0f32; FRAME];
    let mut played: Vec<f32> = Vec::with_capacity(FRAME);
    while !stop.load(Ordering::Relaxed) {
        mic.maintain(|m| m.failed.load(Ordering::Relaxed), open_mic, &notice);
        spk.maintain(|s| s.failed.load(Ordering::Relaxed), open_spk, &notice);

        // Speaker: frames from the core while the device keeps draining; the echo canceller
        // hears exactly what is handed to the ring.
        if let Some(s) = spk.dev.as_mut() {
            while s.queued() < SPK_TARGET && s.prod.slots() >= FRAME {
                let pcm16 = node.pull_speaker();
                played.clear();
                played.extend(pcm16.iter().map(|v| *v as f32 / 32768.0));
                for chunk in played.chunks_exact(APM_FRAME) {
                    if let Err(e) = apm.analyze_render_frame([chunk]) {
                        eprintln!("render apm: {e:?}");
                    }
                }
                played.iter().for_each(|v| { let _ = s.prod.push(*v); });
            }
        }

        // Mic: samples from the ring, cleaned and cut into 20 ms frames. The canceller keeps
        // running while muted so its state stays fresh; only silence is sent.
        if let Some(m) = mic.dev.as_mut() {
            while let Ok(v) = m.cons.pop() {
                pcm.push(v);
            }
        }
        if pcm.len() > RATE as usize {
            pcm.drain(..pcm.len() - RATE as usize);
        }
        while pcm.len() >= FRAME {
            frame.copy_from_slice(&pcm[..FRAME]);
            pcm.drain(..FRAME);
            if !bypass {
                for half in frame.chunks_exact_mut(APM_FRAME) {
                    if let Err(e) = apm.process_capture_frame([&mut *half]) {
                        eprintln!("capture apm: {e:?}");
                    }
                }
            }
            if muted.load(Ordering::Relaxed) {
                frame.fill(0.0);
            }
            node.push_mic(frame.iter().map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16).collect());
        }
        thread::sleep(WORKER_TICK);
    }
}

/// Settings > Sounds "Quiet": no sounds from Tinline (ringing, call tones, cues).
pub static QUIET: AtomicBool = AtomicBool::new(false);
/// Settings > Notifications "Calls": off = no ringing (the call screen still shows).
pub static RING_CALLS: AtomicBool = AtomicBool::new(true);
/// Settings > Sounds: the call sound set (`cues::Palette` as 0/1/2).
pub static PALETTE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub fn palette() -> crate::cues::Palette {
    crate::cues::Palette::ALL[(PALETTE.load(Ordering::Relaxed) as usize).min(2)]
}

/// Loops a call cue (`ring`, `back`, `waiting`; see `cues::call`) until stopped. Quiet plays nothing.
pub fn start_loop(out_dev: Option<String>, kind: &'static str, gain: f64) -> Session {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let join = thread::Builder::new()
        .name("ringer".into())
        .spawn(move || {
            if QUIET.load(Ordering::Relaxed) {
                return;
            }
            let Some(cycle) = crate::cues::call(kind, palette(), gain) else { return };
            let quiet: Notice = Arc::new(|_| {});
            let mut spk = Slot::<Spk>::new("Speaker", out_dev);
            let mut i = 0usize;
            while !flag.load(Ordering::Relaxed) {
                spk.maintain(|s| s.failed.load(Ordering::Relaxed), open_spk, &quiet);
                if let Some(s) = spk.dev.as_mut() {
                    while s.queued() < SPK_TARGET && s.prod.slots() > 0 {
                        let _ = s.prod.push(cycle[i]);
                        i = (i + 1) % cycle.len();
                    }
                }
                thread::sleep(WORKER_TICK);
            }
        })
        .ok();
    Session { stop, join }
}

/// The ringtone, or with `waiting` the soft call-waiting cue over a call (audio.ts: gain 0.6).
pub fn start_ring(out_dev: Option<String>, waiting: bool) -> Session {
    if waiting { start_loop(out_dev, "waiting", 0.6) } else { start_loop(out_dev, "ring", 1.0) }
}

/// A call one-shot (`connected`, `ended`, `failed`, `pulse`, group cues) in the current sound set.
pub fn play_call_cue(out_dev: Option<String>, kind: &str) {
    if let Some(s) = crate::cues::call(kind, palette(), 1.0) {
        play_cue(out_dev, s);
    }
}

/// Plays a one-shot cue (48 kHz mono, see `crate::cues`) on the output device, off the caller's
/// thread. It layers: a cue never stops a ring or a call (each opens its own stream).
pub fn play_cue(out_dev: Option<String>, samples: Vec<f32>) {
    if QUIET.load(Ordering::Relaxed) {
        return;
    }
    let _ = thread::Builder::new().name("cue".into()).spawn(move || {
        let Ok(mut spk) = open_spk(&out_dev) else { return };
        let mut i = 0;
        let deadline = Instant::now() + Duration::from_secs(10);
        while i < samples.len() && Instant::now() < deadline && !spk.failed.load(Ordering::Relaxed) {
            while i < samples.len() && spk.prod.slots() > 0 && spk.queued() < SPK_TARGET * 4 {
                let _ = spk.prod.push(samples[i]);
                i += 1;
            }
            thread::sleep(WORKER_TICK);
        }
        // let the ring drain before the stream closes
        while spk.queued() > 0 && Instant::now() < deadline && !spk.failed.load(Ordering::Relaxed) {
            thread::sleep(WORKER_TICK);
        }
    });
}

/// Call audio and the ringer, driven straight from the core's event callbacks rather than the
/// UI loop: a window the compositor isn't drawing can stall iced for seconds, and that must
/// never delay or mute a call. The UI only edits the settings here and stops the ringer.
#[derive(Default)]
pub struct Ctl {
    node: std::sync::Mutex<std::sync::Weak<Node>>,
    /// (input, output); None = system default.
    pub devices: std::sync::Mutex<(Option<String>, Option<String>)>,
    pub muted: Arc<AtomicBool>,
    /// Test tone sent instead of the mic, if set.
    pub tone: std::sync::Mutex<Option<f32>>,
    /// Tells the UI about device trouble during a call.
    notice: std::sync::Mutex<Option<Notice>>,
    call: std::sync::Mutex<Option<Session>>,
    ring: std::sync::Mutex<Option<Session>>,
    /// The ringback of my outgoing call while theirs rings.
    back: std::sync::Mutex<Option<Session>>,
    /// Incoming calls (they ring here; no ringback for them).
    incoming: std::sync::Mutex<std::collections::HashSet<String>>,
    /// The call whose audio is running, so another call ending does not stop it.
    call_id: std::sync::Mutex<Option<String>>,
    /// The second incoming call whose beep is playing.
    waiting_id: std::sync::Mutex<Option<String>>,
    /// Group-call audio (GA-A1): its own session, so nothing on the 1:1 path (a 1:1 call ending,
    /// the ringer) stops it. `Node::push_mic`/`pull_speaker` reach the group call whenever no
    /// 1:1 call is active.
    group: std::sync::Mutex<Option<Session>>,
}

impl Ctl {
    pub fn attach(&self, node: &Arc<Node>) {
        *self.node.lock().unwrap() = Arc::downgrade(node);
    }

    pub fn set_notice(&self, f: impl Fn(String) + Send + Sync + 'static) {
        *self.notice.lock().unwrap() = Some(Arc::new(f));
    }

    pub fn on_incoming(&self, id: &str) {
        self.incoming.lock().unwrap().insert(id.to_string());
        let waiting = self.call.lock().unwrap().is_some();
        // Settings > Notifications: Calls off = no ringing (a waiting beep over a call still sounds).
        if !waiting && !RING_CALLS.load(Ordering::Relaxed) {
            return;
        }
        // Already talking: the UI shows the waiting call, with a quiet beep instead of a ring.
        *self.waiting_id.lock().unwrap() = waiting.then(|| id.to_string());
        let out = self.devices.lock().unwrap().1.clone();
        let old = self.ring.lock().unwrap().replace(start_ring(out, waiting));
        retire(old);
    }

    /// A group call someone else started rings like an incoming call (the same ring cue). Replaces what rings.
    pub fn ring_group(&self) {
        if !RING_CALLS.load(Ordering::Relaxed) {
            return;
        }
        let out = self.devices.lock().unwrap().1.clone();
        let old = self.ring.lock().unwrap().replace(start_ring(out, false));
        retire(old);
    }

    pub fn stop_ring(&self) {
        retire(self.ring.lock().unwrap().take());
    }

    pub fn on_state(&self, id: &str, state: &p2pcore::CallState) {
        let Some(node) = self.node.lock().unwrap().upgrade() else { return };
        let out = self.devices.lock().unwrap().1.clone();
        let incoming = self.incoming.lock().unwrap().contains(id);
        match state {
            // My call is ringing on their side: the ringback, until they answer or it ends.
            p2pcore::CallState::Ringing if !incoming => {
                if self.back.lock().unwrap().is_none() {
                    *self.back.lock().unwrap() = Some(start_loop(out, "back", 1.0));
                }
            }
            p2pcore::CallState::Active => {
                retire(self.back.lock().unwrap().take());
                if self.call_id.lock().unwrap().as_deref() != Some(id) {
                    play_call_cue(out.clone(), "connected");
                }
                self.stop_ring();
                *self.waiting_id.lock().unwrap() = None;
                *self.call_id.lock().unwrap() = Some(id.to_string());
                self.muted.store(false, Ordering::Relaxed);
                node.set_test_tone(*self.tone.lock().unwrap());
                let (i, o) = self.devices.lock().unwrap().clone();
                let notice = self.notice.lock().unwrap().clone().unwrap_or_else(|| Arc::new(|_| {}));
                let s = start_call(node, i, o, self.muted.clone(), notice);
                retire(self.call.lock().unwrap().replace(s));
            }
            p2pcore::CallState::Ended { reason } => {
                self.incoming.lock().unwrap().remove(id);
                retire(self.back.lock().unwrap().take());
                // The second call giving up (or declined) leaves the call it rang over alone.
                if self.waiting_id.lock().unwrap().as_deref() == Some(id) {
                    *self.waiting_id.lock().unwrap() = None;
                    self.stop_ring();
                    return;
                }
                if self.call_id.lock().unwrap().as_deref().is_some_and(|c| c != id) {
                    return;
                }
                let was_on = self.call_id.lock().unwrap().take().is_some();
                self.stop_all();
                node.set_test_tone(None);
                // audio.ts: `ended` after a call, `failed` when my call never got through.
                if was_on {
                    play_call_cue(out, "ended");
                } else if !incoming && matches!(reason.as_str(), "unreachable" | "failed" | "error") {
                    play_call_cue(out, "failed");
                }
            }
            _ => {}
        }
    }

    /// Any group-call event: runs the mic and speaker while this device's user is in a group
    /// call, stops them once not. Reads the core's state instead of trusting the event (the
    /// callbacks run on separate threads and can arrive out of order, e.g. `Left` of a
    /// superseded call after `Joined` of the one that won).
    pub fn on_group_call(&self) {
        let Some(node) = self.node.lock().unwrap().upgrade() else { return };
        let want = node.group_call_here().is_some_and(|h| h.in_call);
        let mut slot = self.group.lock().unwrap();
        if want && slot.is_none() {
            let (i, o) = self.devices.lock().unwrap().clone();
            let notice = self.notice.lock().unwrap().clone().unwrap_or_else(|| Arc::new(|_| {}));
            // Muting in a group call is the core's (`group_call_set_muted`); this flag stays clear.
            *slot = Some(start_call(node, i, o, Arc::new(AtomicBool::new(false)), notice));
        } else if !want {
            retire(slot.take());
        }
    }

    pub fn stop_all(&self) {
        self.stop_ring();
        retire(self.call.lock().unwrap().take());
    }
}

/// Tears a session down off the caller's thread (joining cpal can take a moment, and the
/// caller may be a core callback).
fn retire(s: Option<Session>) {
    if let Some(s) = s {
        let _ = thread::Builder::new().name("audio-stop".into()).spawn(move || drop(s));
    }
}
