//! Opt-in simulation recorder. No raw PCM is written. Not a production callback API.
use crate::spectral::{Extractor, Features};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufWriter, Write};

struct Stream {
    extractor: Extractor,
    active: bool,
}
pub struct Recorder {
    receiver: u8,
    out: BufWriter<File>,
    streams: BTreeMap<(u8, &'static str), Stream>,
}
impl Recorder {
    pub fn new(path: &str, receiver: u8) -> Self {
        Self {
            receiver,
            out: BufWriter::new(File::create(path).expect("create feature trace")),
            streams: BTreeMap::new(),
        }
    }
    fn row(
        &mut self,
        kind: &str,
        source: u8,
        level: u8,
        audible: bool,
        eligible: bool,
        at: u64,
        f: Features,
    ) {
        writeln!(
            self.out,
            "{at} {} {source} {kind} {level} {} {} {:.9} {:.9} {:.9} {:.6} {:.9}",
            self.receiver,
            u8::from(audible),
            u8::from(eligible),
            f.low,
            f.mid,
            f.high,
            f.centroid_hz,
            f.onset
        )
        .expect("write diagnostic features");
    }
    pub fn record(
        &mut self,
        kind: &'static str,
        source: u8,
        receiver: u8,
        level: u8,
        audible: bool,
        eligible: bool,
        at: u64,
        pcm: &[i16],
    ) {
        if receiver != self.receiver {
            return;
        }
        let key = (source, kind);
        if !audible || !eligible {
            if let Some(stream) = self.streams.get_mut(&key) {
                if stream.active {
                    stream.extractor.reset();
                    stream.active = false;
                    self.row(kind, source, level, audible, false, at, Features::default());
                }
            }
            return; // no FFT for silent/unknown frames, no allocation for a never-audible source
        }
        if !self.streams.contains_key(&key) && self.streams.len() >= crate::MAX_SLOTS {
            return;
        }
        let stream = self.streams.entry(key).or_insert_with(|| Stream {
            extractor: Extractor::new(),
            active: false,
        });
        stream.active = true;
        if let Some(features) = stream.extractor.push(pcm) {
            self.row(kind, source, level, true, true, at, features);
        }
    }
    pub fn finish(&mut self, receiver: u8, at: u64) {
        if receiver == self.receiver {
            self.row("end", receiver, 127, false, false, at, Features::default());
        }
    }
    pub fn flush(&mut self) {
        self.out.flush().expect("flush features");
    }
}
