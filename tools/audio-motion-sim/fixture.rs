//! Same deterministic PCM fixture for offline previews and the live Iced viewer.
use crate::{
    motion::{Model, ShapeFeatures},
    spectral,
};

pub fn synthetic(m: &mut Model, extractors: &mut [spectral::Extractor; 4], sample: &mut u64) {
    let t = m.time();
    if t >= 18.0 {
        m.end_call();
        return;
    }
    if (14.0..16.0).contains(&t) {
        return;
    }
    if t >= 16.0 {
        m.set_roster(&[0, 2]);
    }
    for (source, extractor) in extractors.iter_mut().enumerate() {
        let (level, audible, hz) = if t >= 16.0 {
            (32, source == 2, 1200.0)
        } else if source > 0 {
            (
                24 + source as u8 * 5,
                (8.0..12.0).contains(&t),
                [0.0, 350.0, 1200.0, 4000.0][source],
            )
        } else if t < 2.0 || (6.0..7.0).contains(&t) {
            (127, false, 200.0)
        } else if t < 6.0 {
            (35, t % 1.4 < 1.0, if t < 4.0 { 200.0 } else { 4000.0 })
        } else if t < 8.0 {
            (5, true, 4000.0)
        } else {
            (30, true, 200.0)
        };
        let amplitude = if audible {
            32767.0 * 10.0_f64.powf(-(level as f64) / 20.0) * 2.0_f64.sqrt()
        } else {
            0.0
        };
        let pcm: [i16; 1600] = std::array::from_fn(|i| {
            (amplitude
                * (std::f64::consts::TAU * hz * (*sample + i as u64) as f64 / spectral::RATE).sin())
                as i16
        });
        if let Some(f) = extractor.push(&pcm) {
            m.feed_spectrum(
                source,
                level,
                audible,
                (12.0..16.0).contains(&t),
                t,
                ShapeFeatures {
                    low: f.low,
                    high: f.high,
                    centroid_hz: f.centroid_hz,
                    onset: f.onset,
                },
            );
        }
    }
    *sample += 1600;
}

/// Same nominal RMS/level for low, high and mixed frequency controls.
pub fn tone(
    m: &mut Model,
    extractor: &mut spectral::Extractor,
    sample: &mut u64,
    low: bool,
    high: bool,
) {
    let pcm = tone_pcm(*sample, low, high);
    if let Some(f) = extractor.push(&pcm) {
        m.feed_spectrum(
            0,
            35,
            true,
            false,
            m.time(),
            ShapeFeatures {
                low: f.low,
                high: f.high,
                centroid_hz: f.centroid_hz,
                onset: f.onset,
            },
        );
    }
    *sample += 1600;
}

/// Shared PCM input for both native visual treatments, without playback.
pub fn tone_pcm(sample: u64, low: bool, high: bool) -> [i16; 1600] {
    let amplitude = 32767.0 * 10.0_f64.powf(-35.0 / 20.0) * 2.0_f64.sqrt();
    let gain = if low && high {
        1.0 / 2.0_f64.sqrt()
    } else {
        1.0
    };
    std::array::from_fn(|i| {
        let phase = std::f64::consts::TAU * (sample + i as u64) as f64 / spectral::RATE;
        (amplitude
            * gain
            * (if low { (phase * 200.0).sin() } else { 0.0 }
                + if high { (phase * 4000.0).sin() } else { 0.0 })) as i16
    })
}
