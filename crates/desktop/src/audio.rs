//! Audio devices for calls: cpal capture/playback at 48 kHz mono as far as the hardware
//! allows (anything else is converted), WebRTC audio processing (echo cancellation, noise
//! suppression, high-pass) on the mic with the render reference fed from the playback side,
//! 20 ms framing to and from the core, and a ring tone.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};
use p2pcore::Node;
use webrtc_audio_processing::config::{Config, EchoCanceller, HighPassFilter, NoiseSuppression};
use webrtc_audio_processing::Processor;

const RATE: u32 = audio::RATE;
const FRAME: usize = audio::FRAME;
/// WebRTC audio processing works on 10 ms frames.
const APM_FRAME: usize = 480;

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

/// Prefers 48 kHz with the fewest channels and f32 samples; otherwise the device default
/// (the stream is then resampled).
fn pick_config(dev: &Device, input: bool) -> Result<(StreamConfig, SampleFormat), String> {
    let ranges: Vec<_> = if input {
        dev.supported_input_configs().map_err(|e| e.to_string())?.collect()
    } else {
        dev.supported_output_configs().map_err(|e| e.to_string())?.collect()
    };
    let best = ranges
        .into_iter()
        .filter(|r| r.contains_rate(RATE) && matches!(r.sample_format(), SampleFormat::F32 | SampleFormat::I16))
        .min_by_key(|r| (r.channels(), r.sample_format() != SampleFormat::F32));
    if let Some(r) = best {
        let c = r.with_sample_rate(RATE);
        return Ok((c.config(), c.sample_format()));
    }
    let c = if input { dev.default_input_config() } else { dev.default_output_config() }
        .map_err(|e| e.to_string())?;
    Ok((c.config(), c.sample_format()))
}

/// Streaming linear resampler.
struct Lin {
    step: f64,
    pos: f64,
    last: f32,
}

impl Lin {
    fn new(from: u32, to: u32) -> Self {
        Self { step: from as f64 / to as f64, pos: 0.0, last: 0.0 }
    }

    fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if input.is_empty() {
            return;
        }
        let mut buf = Vec::with_capacity(input.len() + 1);
        buf.push(self.last);
        buf.extend_from_slice(input);
        while self.pos + 1.0 < buf.len() as f64 {
            let i = self.pos as usize;
            let f = (self.pos - i as f64) as f32;
            out.push(buf[i] * (1.0 - f) + buf[i + 1] * f);
            self.pos += self.step;
        }
        self.pos -= (buf.len() - 1) as f64;
        self.last = *buf.last().unwrap();
    }
}

/// Opens the mic; `sink` receives mono 48 kHz samples in chunks of any size.
fn open_input(
    name: &Option<String>,
    mut sink: impl FnMut(&[f32]) + Send + 'static,
) -> Result<Stream, String> {
    let dev = find(name, true).ok_or("no input device")?;
    let (cfg, fmt) = pick_config(&dev, true)?;
    let ch = cfg.channels as usize;
    let mut lin = (cfg.sample_rate != RATE).then(|| Lin::new(cfg.sample_rate, RATE));
    let (mut mono, mut out) = (Vec::new(), Vec::new());
    let mut feed = move |frames: &mut dyn Iterator<Item = f32>| {
        mono.clear();
        let it = frames;
        loop {
            let mut sum = 0.0;
            let mut n = 0;
            for _ in 0..ch {
                match it.next() {
                    Some(s) => {
                        sum += s;
                        n += 1;
                    }
                    None => break,
                }
            }
            if n < ch {
                break;
            }
            mono.push(sum / ch as f32);
        }
        match &mut lin {
            Some(l) => {
                out.clear();
                l.process(&mono, &mut out);
                sink(&out);
            }
            None => sink(&mono),
        }
    };
    let err = |e: cpal::Error| eprintln!("mic: {e}");
    let stream = match fmt {
        SampleFormat::F32 => dev.build_input_stream(
            cfg,
            move |d: &[f32], _: &_| feed(&mut d.iter().copied()),
            err,
            None,
        ),
        SampleFormat::I16 => dev.build_input_stream(
            cfg,
            move |d: &[i16], _: &_| feed(&mut d.iter().map(|s| *s as f32 / 32768.0)),
            err,
            None,
        ),
        other => return Err(format!("unsupported mic sample format {other:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

/// Opens the speaker; `producer` appends mono 48 kHz samples whenever the device wants more.
fn open_output(
    name: &Option<String>,
    mut producer: impl FnMut(&mut Vec<f32>) + Send + 'static,
) -> Result<Stream, String> {
    let dev = find(name, false).ok_or("no output device")?;
    let (cfg, fmt) = pick_config(&dev, false)?;
    let ch = cfg.channels as usize;
    let mut lin = (cfg.sample_rate != RATE).then(|| Lin::new(cfg.sample_rate, RATE));
    let mut ready: VecDeque<f32> = VecDeque::new();
    let (mut chunk, mut conv) = (Vec::new(), Vec::new());
    let mut fill = move |frames: usize, put: &mut dyn FnMut(usize, f32)| {
        let mut guard = 0;
        while ready.len() < frames && guard < 16 {
            guard += 1;
            chunk.clear();
            producer(&mut chunk);
            match &mut lin {
                Some(l) => {
                    conv.clear();
                    l.process(&chunk, &mut conv);
                    ready.extend(conv.iter());
                }
                None => ready.extend(chunk.iter()),
            }
        }
        for i in 0..frames {
            put(i, ready.pop_front().unwrap_or(0.0));
        }
    };
    let err = |e: cpal::Error| eprintln!("speaker: {e}");
    let stream = match fmt {
        SampleFormat::F32 => dev.build_output_stream(
            cfg,
            move |d: &mut [f32], _: &_| {
                fill(d.len() / ch, &mut |i, s| d[i * ch..(i + 1) * ch].fill(s));
            },
            err,
            None,
        ),
        SampleFormat::I16 => dev.build_output_stream(
            cfg,
            move |d: &mut [i16], _: &_| {
                fill(d.len() / ch, &mut |i, s| {
                    d[i * ch..(i + 1) * ch].fill((s.clamp(-1.0, 1.0) * 32767.0) as i16)
                });
            },
            err,
            None,
        ),
        other => return Err(format!("unsupported speaker sample format {other:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
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
) -> Session {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let join = thread::Builder::new()
        .name("call-audio".into())
        .spawn(move || run_call(node, in_dev, out_dev, muted, flag))
        .ok();
    Session { stop, join }
}

fn run_call(
    node: Arc<Node>,
    in_dev: Option<String>,
    out_dev: Option<String>,
    muted: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) {
    let apm = match Processor::new(RATE) {
        Ok(p) => Arc::new(p),
        Err(e) => {
            eprintln!("audio processing unavailable: {e:?}");
            return;
        }
    };
    apm.set_config(Config {
        echo_canceller: Some(EchoCanceller::default()),
        high_pass_filter: Some(HighPassFilter::default()),
        noise_suppression: Some(NoiseSuppression::default()),
        ..Default::default()
    });

    // Speaker: frames from the core; the echo canceller hears exactly what is handed to the
    // device.
    let (render_apm, render_node) = (apm.clone(), node.clone());
    let mut played: Vec<f32> = Vec::new();
    let speaker = open_output(&out_dev, move |out| {
        let pcm = render_node.pull_speaker();
        let start = out.len();
        out.extend(pcm.iter().map(|s| *s as f32 / 32768.0));
        played.extend_from_slice(&out[start..]);
        while played.len() >= APM_FRAME {
            if let Err(e) = render_apm.analyze_render_frame([&played[..APM_FRAME]]) {
                eprintln!("render apm: {e:?}");
            }
            played.drain(..APM_FRAME);
        }
    });
    if let Err(e) = &speaker {
        eprintln!("speaker: {e}");
    }

    // Mic: chunks from the device callback, cleaned and cut into 20 ms frames on this thread.
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let mic = open_input(&in_dev, move |d| drop(tx.send(d.to_vec())));
    if let Err(e) = &mic {
        eprintln!("mic: {e}");
    }
    let mut pcm: Vec<f32> = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(chunk) => pcm.extend(chunk),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => thread::sleep(Duration::from_millis(50)),
        }
        while pcm.len() >= FRAME {
            let mut frame: Vec<f32> = pcm.drain(..FRAME).collect();
            if muted.load(Ordering::Relaxed) {
                frame.fill(0.0);
            } else {
                for half in frame.chunks_exact_mut(APM_FRAME) {
                    if let Err(e) = apm.process_capture_frame([&mut *half]) {
                        eprintln!("capture apm: {e:?}");
                    }
                }
            }
            node.push_mic(frame.iter().map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16).collect());
        }
    }
    drop(mic);
    drop(speaker);
}

/// Plays a classic double-ring until stopped.
pub fn start_ring(out_dev: Option<String>) -> Session {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let join = thread::Builder::new()
        .name("ringer".into())
        .spawn(move || {
            let mut n: u64 = 0;
            let stream = open_output(&out_dev, move |out| {
                // 20 ms per call; 2 s cycle: two 0.4 s bursts, 0.2 s apart, then silence.
                for i in 0..FRAME as u64 {
                    let t = n * FRAME as u64 + i;
                    let ms = (t * 1000 / RATE as u64) % 2000;
                    let on = ms < 400 || (600..1000).contains(&ms);
                    let ph = t as f32 / RATE as f32;
                    let env = if on { 1.0 } else { 0.0 };
                    let s = ((ph * 440.0 * std::f32::consts::TAU).sin()
                        + (ph * 480.0 * std::f32::consts::TAU).sin())
                        * 0.12
                        * env;
                    out.push(s);
                }
                n += 1;
            });
            if let Err(e) = &stream {
                eprintln!("ring: {e}");
            }
            while !flag.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(50));
            }
        })
        .ok();
    Session { stop, join }
}
