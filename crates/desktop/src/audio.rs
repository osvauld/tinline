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

/// One 20 ms slice of the ring tone: a classic double-ring on a 2 s cycle (two 0.4 s bursts,
/// 0.2 s apart, then silence). `n` counts slices.
fn ring_frame(n: u64, out: &mut Vec<f32>) {
    for i in 0..FRAME as u64 {
        let t = n * FRAME as u64 + i;
        let ms = (t * 1000 / RATE as u64) % 2000;
        let on = ms < 400 || (600..1000).contains(&ms);
        let ph = t as f32 / RATE as f32;
        let env = if on { 1.0 } else { 0.0 };
        out.push(
            ((ph * 440.0 * std::f32::consts::TAU).sin() + (ph * 480.0 * std::f32::consts::TAU).sin())
                * 0.12
                * env,
        );
    }
}

/// Plays a classic double-ring until stopped.
pub fn start_ring(out_dev: Option<String>) -> Session {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let join = thread::Builder::new()
        .name("ringer".into())
        .spawn(move || {
            let quiet: Notice = Arc::new(|_| {});
            let mut spk = Slot::<Spk>::new("Speaker", out_dev);
            let (mut n, mut buf) = (0u64, Vec::with_capacity(FRAME));
            while !flag.load(Ordering::Relaxed) {
                spk.maintain(|s| s.failed.load(Ordering::Relaxed), open_spk, &quiet);
                if let Some(s) = spk.dev.as_mut() {
                    while s.queued() < SPK_TARGET && s.prod.slots() >= FRAME {
                        buf.clear();
                        ring_frame(n, &mut buf);
                        n += 1;
                        buf.iter().for_each(|v| { let _ = s.prod.push(*v); });
                    }
                }
                thread::sleep(WORKER_TICK);
            }
        })
        .ok();
    Session { stop, join }
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
}

impl Ctl {
    pub fn attach(&self, node: &Arc<Node>) {
        *self.node.lock().unwrap() = Arc::downgrade(node);
    }

    pub fn set_notice(&self, f: impl Fn(String) + Send + Sync + 'static) {
        *self.notice.lock().unwrap() = Some(Arc::new(f));
    }

    pub fn on_incoming(&self) {
        if self.call.lock().unwrap().is_some() {
            return; // already talking; the UI shows the waiting call
        }
        let out = self.devices.lock().unwrap().1.clone();
        let old = self.ring.lock().unwrap().replace(start_ring(out));
        retire(old);
    }

    pub fn stop_ring(&self) {
        retire(self.ring.lock().unwrap().take());
    }

    pub fn on_state(&self, state: &p2pcore::CallState) {
        let Some(node) = self.node.lock().unwrap().upgrade() else { return };
        match state {
            p2pcore::CallState::Active => {
                self.stop_ring();
                self.muted.store(false, Ordering::Relaxed);
                node.set_test_tone(*self.tone.lock().unwrap());
                let (i, o) = self.devices.lock().unwrap().clone();
                let notice = self.notice.lock().unwrap().clone().unwrap_or_else(|| Arc::new(|_| {}));
                let s = start_call(node, i, o, self.muted.clone(), notice);
                retire(self.call.lock().unwrap().replace(s));
            }
            p2pcore::CallState::Ended { .. } => {
                self.stop_all();
                node.set_test_tone(None);
            }
            _ => {}
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
