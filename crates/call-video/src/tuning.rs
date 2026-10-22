//! Session-only developer controls. Apps hide these in production builds; defaults do nothing.
//! The record, its validation and the sender configuration are `video_call_core::tuning`; this
//! builds the offer and holds the node's settings.
use super::*;

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

impl NodeVideo {
    /// The settings the next call's offer is built from.
    pub fn tuning(&self) -> VideoTuning {
        self.tuning.lock().clone()
    }

    /// Session-only. Changes never mutate an offer or encoder already in flight.
    pub fn set_tuning(&self, tuning: VideoTuning) -> Result<(), Error> {
        tuning.validate(&codecs::local_offer_codecs()).map_err(|e| Error::Protocol(e.to_string()))?;
        *self.tuning.lock() = tuning;
        Ok(())
    }

    /// The send codecs a tuning may name: "auto" and each this device offers.
    pub fn tuning_codecs(&self) -> Vec<String> {
        std::iter::once("auto".into())
            .chain(
                codecs::local_offer_codecs()
                    .encode
                    .into_iter()
                    .map(|c| c.as_str().into()),
            )
            .collect()
    }

    /// Our offer for `call`, when video is enabled on this node.
    pub fn offer_for(&self, call: &CallVideo) -> Option<VideoOffer> {
        self.offer(&call.pipe)
    }
}

#[cfg(test)]
mod tests;
