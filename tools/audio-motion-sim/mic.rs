//! Opt-in default microphone. Bounded capture queue; no recording, output or networking.
//! Callback only converts/queues PCM. FFT/RMS happen in poll(), outside that callback.
use crate::spectral::{Extractor, Features};
use cpal::{
    BufferSize, FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig,
    SupportedBufferSize,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

const FRAME: usize = 960;
const BLOCKS: usize = 8; // <=160ms PCM; old blocks rejected rather than replayed as live
const MAX_AGE: Duration = Duration::from_millis(100);
#[derive(Debug)]
struct Block {
    pcm: [i16; FRAME],
    at: Instant,
}
struct Assembler {
    pcm: [i16; FRAME],
    filled: usize,
}
impl Assembler {
    fn new() -> Self {
        Self {
            pcm: [0; FRAME],
            filled: 0,
        }
    }
    fn push(&mut self, value: i16, out: &mut rtrb::Producer<Block>, drops: &AtomicU64) {
        self.pcm[self.filled] = value;
        self.filled += 1;
        if self.filled == FRAME {
            if out
                .push(Block {
                    pcm: self.pcm,
                    at: Instant::now(),
                })
                .is_err()
            {
                drops.fetch_add(1, Ordering::Relaxed);
            }
            self.filled = 0;
        }
    }
}
fn mono<T: Sample + Copy>(frame: &[T]) -> i16
where
    f32: FromSample<T>,
{
    let value = frame.iter().map(|&s| f32::from_sample(s)).sum::<f32>() / frame.len().max(1) as f32;
    if value.is_finite() {
        (value.clamp(-1.0, 1.0) * 32767.0) as i16
    } else {
        0
    }
}
pub struct Observation {
    pub level: u8,
    pub audible: bool,
    pub features: Option<Features>,
    pub at: Instant,
}
struct Analysis {
    input: rtrb::Consumer<Block>,
    extractor: Extractor,
    latest: Option<Features>,
    last: Option<Instant>,
    drops: Arc<AtomicU64>,
    previous_drops: u64,
}
impl Analysis {
    fn new(input: rtrb::Consumer<Block>, drops: Arc<AtomicU64>) -> Self {
        Self {
            input,
            extractor: Extractor::new(),
            latest: None,
            last: None,
            drops,
            previous_drops: 0,
        }
    }
    fn reset(&mut self) {
        self.extractor.reset();
        self.latest = None;
        self.last = None;
    }
    fn fresh(&self) -> bool {
        self.last
            .is_some_and(|at| at.elapsed() <= Duration::from_millis(200))
    }
    fn poll(&mut self) -> Option<Observation> {
        let drops = self.drops.load(Ordering::Relaxed);
        if drops != self.previous_drops {
            self.reset();
            self.previous_drops = drops;
        }
        let mut observation = None;
        for _ in 0..BLOCKS {
            // bounded even if producer continues writing during this poll
            let Ok(block) = self.input.pop() else {
                break;
            };
            if block.at.elapsed() > MAX_AGE {
                self.reset();
                continue;
            }
            let level = audio::level(&block.pcm); // existing source-level computation
            let audible = audio::rms(&block.pcm) > 200.0; // experimental quiet gate, not call policy
            if audible {
                if let Some(f) = self.extractor.push(&block.pcm) {
                    self.latest = Some(f);
                }
            } else {
                self.reset();
            }
            self.last = Some(block.at);
            observation = Some(Observation {
                level,
                audible,
                features: self.latest,
                at: block.at,
            });
        }
        observation
    }
}
fn build<T>(
    device: &cpal::Device,
    cfg: StreamConfig,
    mut out: rtrb::Producer<Block>,
    drops: Arc<AtomicU64>,
    failed: Arc<AtomicBool>,
) -> Result<Stream, String>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = cfg.channels as usize;
    let mut assembler = Assembler::new();
    device
        .build_input_stream(
            cfg,
            move |data: &[T], _: &_| {
                for frame in data.chunks_exact(channels) {
                    assembler.push(mono(frame), &mut out, &drops);
                }
            },
            move |_| {
                failed.store(true, Ordering::Relaxed);
            },
            None,
        )
        .map_err(|e| e.to_string())
}
pub struct Mic {
    _stream: Stream,
    analysis: Analysis,
    failed: Arc<AtomicBool>,
}
impl Mic {
    pub fn start() -> Result<Self, String> {
        let device = cpal::default_host()
            .default_input_device()
            .ok_or("No default microphone found")?;
        let default = device.default_input_config().map_err(|e| e.to_string())?;
        let info = if default.sample_rate() == 48_000 {
            default
        } else {
            device
                .supported_input_configs()
                .map_err(|e| e.to_string())?
                .find(|c| {
                    c.min_sample_rate() <= 48_000
                        && c.max_sample_rate() >= 48_000
                        && matches!(
                            c.sample_format(),
                            SampleFormat::F32 | SampleFormat::I16 | SampleFormat::I32
                        )
                })
                .ok_or("Microphone does not expose 48kHz input; this experiment does not resample")?
                .with_sample_rate(48_000)
        };
        let mut cfg = info.config();
        if cfg.channels == 0 || cfg.channels > 32 {
            return Err("Unsupported microphone channel count".into());
        }
        if let SupportedBufferSize::Range { min, max } = *info.buffer_size() {
            if min <= FRAME as u32 && max >= FRAME as u32 {
                cfg.buffer_size = BufferSize::Fixed(FRAME as u32);
            }
        }
        let (out, input) = rtrb::RingBuffer::new(BLOCKS);
        let drops = Arc::new(AtomicU64::new(0));
        let failed = Arc::new(AtomicBool::new(false));
        let stream = match info.sample_format() {
            SampleFormat::F32 => build::<f32>(&device, cfg, out, drops.clone(), failed.clone()),
            SampleFormat::F64 => build::<f64>(&device, cfg, out, drops.clone(), failed.clone()),
            SampleFormat::I8 => build::<i8>(&device, cfg, out, drops.clone(), failed.clone()),
            SampleFormat::I16 => build::<i16>(&device, cfg, out, drops.clone(), failed.clone()),
            SampleFormat::I32 => build::<i32>(&device, cfg, out, drops.clone(), failed.clone()),
            SampleFormat::U8 => build::<u8>(&device, cfg, out, drops.clone(), failed.clone()),
            SampleFormat::U16 => build::<u16>(&device, cfg, out, drops.clone(), failed.clone()),
            SampleFormat::U32 => build::<u32>(&device, cfg, out, drops.clone(), failed.clone()),
            other => Err(format!("Unsupported microphone sample format {other}")),
        }?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Self {
            _stream: stream,
            analysis: Analysis::new(input, drops),
            failed,
        })
    }
    pub fn poll(&mut self) -> Option<Observation> {
        self.analysis.poll()
    }
    pub fn fresh(&self) -> bool {
        self.analysis.fresh()
    }
    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }
    pub fn drops(&self) -> u64 {
        self.analysis.drops.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_callbacks_form_exactly_one_block() {
        let (mut p, mut c) = rtrb::RingBuffer::new(BLOCKS);
        let drops = AtomicU64::new(0);
        let mut a = Assembler::new();
        for _ in 0..FRAME - 1 {
            a.push(1000, &mut p, &drops);
        }
        assert!(c.pop().is_err());
        a.push(1000, &mut p, &drops);
        assert_eq!(c.pop().unwrap().pcm, [1000; FRAME]);
        assert!(c.pop().is_err());
    }
    #[test]
    fn overflow_is_counted_without_growing_the_queue() {
        let (mut p, mut c) = rtrb::RingBuffer::new(BLOCKS);
        let drops = AtomicU64::new(0);
        let mut a = Assembler::new();
        for _ in 0..FRAME * (BLOCKS + 1) {
            a.push(1000, &mut p, &drops);
        }
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        for _ in 0..BLOCKS {
            assert!(c.pop().is_ok());
        }
        assert!(c.pop().is_err());
    }
    #[test]
    fn stale_capture_is_not_promoted_to_live_input() {
        let (mut p, c) = rtrb::RingBuffer::new(BLOCKS);
        p.push(Block {
            pcm: [3000; FRAME],
            at: Instant::now() - Duration::from_secs(1),
        })
        .unwrap();
        let mut a = Analysis::new(c, Arc::new(AtomicU64::new(0)));
        assert!(a.poll().is_none());
        assert!(!a.fresh());
    }
    #[test]
    fn level_reuses_audio_rms_and_silence_clears_audibility() {
        let (mut p, c) = rtrb::RingBuffer::new(BLOCKS);
        p.push(Block {
            pcm: [3000; FRAME],
            at: Instant::now(),
        })
        .unwrap();
        let mut a = Analysis::new(c, Arc::new(AtomicU64::new(0)));
        let o = a.poll().unwrap();
        assert_eq!(o.level, audio::level(&[3000; FRAME]));
        assert!(o.audible);
        p.push(Block {
            pcm: [0; FRAME],
            at: Instant::now(),
        })
        .unwrap();
        let o = a.poll().unwrap();
        assert_eq!(o.level, 127);
        assert!(!o.audible);
    }
    #[test]
    fn stereo_and_nonfinite_samples_are_safe() {
        assert_eq!(mono(&[1000i16, -1000i16]), 0);
        assert_eq!(mono(&[f32::NAN]), 0);
        assert_eq!(mono(&[2.0_f32]), 32767);
    }
}
