//! Session-only developer controls. Apps hide these in production builds; defaults do nothing.
//! The record, its validation and the sender configuration are `video_call_core::tuning`; this
//! declares the record to UniFFI, builds the offer and exports the node's settings calls.
use super::*;

#[uniffi::remote(Record)]
pub struct VideoTuning {
    /// Empty or "auto", "av1", or "vp8". Applies to our sending direction only.
    pub send_codec: String,
    /// Zero = normal adaptation; otherwise a ceiling (not a promised wire rate).
    pub max_bps: u32,
    /// Zero = adaptive; otherwise 360, 540 or 720. Peer/budget limits still win.
    pub fixed_height: u32,
    /// Empty or "auto", "hw" or "sw": which decoder receives the peer's video, where this device
    /// has both kinds for the codec that arrives (VP8 on a phone with a hardware decoder). Local
    /// only: the offer's `hw_decode` (and so the peer's codec choice) is unchanged.
    pub recv_decoder: String,
    /// Empty, "auto" or "probe": Cut+probe (`SenderConfig::smooth_cut` and `burst_room`: probing,
    /// where the peer's offer says `burst_probe`), the default since 2026-10-20 (it fixed the
    /// moto's stall on a bumpy upload). For A/B calls (`docs/design/video-sim-tuning.md`): "plain"
    /// (the controller before, neither), "cut" (`smooth_cut` only) or "trend" (Cut+probe and the
    /// delay-trend detector).
    pub rate_control: String,
}

impl NodeVideo {
    pub(super) fn offer(&self, pipe: &Pipe) -> Option<VideoOffer> {
        self.enabled.load(Ordering::SeqCst).then(|| {
            let tuning = self.tuning.lock().clone();
            pipe.stats.lock().tuning = tuning.clone();
            let mut c = codecs::local_offer_codecs();
            tuning.filter(&mut c);
            VideoOffer {
                max_pixels: MAX_PIXELS,
                max_fps: FPS as u8,
                decode: c.decode,
                encode: c.encode,
                hw_decode: c.hw_decode,
                hw_encode: c.hw_encode,
                hint_flags: true,
                codec_switch: true,
                burst_probe: true,
            }
        })
    }
}

#[uniffi::export]
impl Node {
    pub fn video_tuning(&self) -> VideoTuning {
        self.inner.video.tuning.lock().clone()
    }

    /// Session-only. Changes never mutate an offer or encoder already in flight.
    pub fn set_video_tuning(&self, tuning: VideoTuning) -> Result<(), Error> {
        tuning.validate(&codecs::local_offer_codecs()).map_err(|e| Error::Protocol(e.to_string()))?;
        *self.inner.video.tuning.lock() = tuning;
        Ok(())
    }

    pub fn video_tuning_codecs(&self) -> Vec<String> {
        std::iter::once("auto".into())
            .chain(
                codecs::local_offer_codecs()
                    .encode
                    .into_iter()
                    .map(|c| c.as_str().into()),
            )
            .collect()
    }
}

/// Shared developer readout (Settings > Diagnostics while a call runs): the in-call sheet's rows
/// (`video_stat_rows`) without the connection, one "Label value" per line. Payload rate is
/// measured, not the encoder target or QUIC wire rate.
#[uniffi::export]
pub fn video_tuning_stats_label(s: VideoStats) -> String {
    video_call_core::tuning::tuning_stats_label(s)
}

#[cfg(test)]
mod tests;
