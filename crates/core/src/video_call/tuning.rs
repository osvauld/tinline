//! Session-only developer controls. Apps hide these in production builds; defaults do nothing.
use super::*;

#[derive(Debug, Clone, Default, PartialEq, Eq, uniffi::Record)]
pub struct VideoTuning {
    /// Empty or "auto", "av1", or "vp8". Applies to our sending direction only.
    pub send_codec: String,
    /// Zero = normal adaptation; otherwise a ceiling (not a promised wire rate).
    pub max_bps: u32,
    /// Zero = adaptive; otherwise 360, 540 or 720. Peer/budget limits still win.
    pub fixed_height: u32,
}

impl VideoTuning {
    fn codec(&self) -> Option<VideoCodec> {
        match self.send_codec.as_str() {
            "av1" => Some(VideoCodec::Av1),
            "vp8" => Some(VideoCodec::Vp8),
            _ => None,
        }
    }

    fn validate(&self, c: &codecs::LocalCodecs) -> Result<(), Error> {
        if !matches!(self.send_codec.as_str(), "" | "auto" | "av1" | "vp8") {
            return Err(Error::Protocol("unknown send codec".into()));
        }
        if self.codec().is_some_and(|codec| !c.encode.contains(&codec)) {
            return Err(Error::Protocol(
                "this device cannot encode the requested codec".into(),
            ));
        }
        if !matches!(
            self.max_bps,
            0 | 500_000 | 1_000_000 | 2_000_000 | 3_000_000
        ) || !matches!(self.fixed_height, 0 | 360 | 540 | 720)
        {
            return Err(Error::Protocol(
                "unsupported video tuning rate or resolution".into(),
            ));
        }
        Ok(())
    }

    pub(super) fn filter(&self, c: &mut codecs::LocalCodecs) {
        if let Some(codec) = self.codec() {
            c.encode.retain(|&v| v == codec);
            c.hw_encode.retain(|&v| v == codec);
        }
    }

    pub(super) fn configure(&self, cfg: &mut SenderConfig) {
        if self.max_bps != 0 {
            cfg.max_bps = self.max_bps;
            cfg.start_bps = cfg.start_bps.min(cfg.max_bps);
            for rung in &mut cfg.ladder.rungs {
                rung.max_bps = self.max_bps;
            }
        }
        cfg.ladder.fixed_rung = match self.fixed_height {
            360 => Some(1),
            540 => Some(2),
            720 => Some(3),
            _ => None,
        };
    }
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
        tuning.validate(&codecs::local_offer_codecs())?;
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

/// Shared developer readout. Payload rate is measured, not the encoder target or QUIC wire rate.
#[uniffi::export]
pub fn video_tuning_stats_label(s: VideoStats) -> String {
    if !s.tuning_problem.is_empty() {
        return s.tuning_problem;
    }
    let size = if s.tuning.fixed_height == 0 {
        "adaptive".into()
    } else {
        format!("{}p", s.tuning.fixed_height)
    };
    let cap = if s.tuning.max_bps == 0 {
        "auto".into()
    } else {
        format!("{} kbps", s.tuning.max_bps / 1000)
    };
    format!(
        "Send {} {}x{} @{} fps · payload {} kbps (target {}) · enc {:.1} ms\nRecv {} {}x{} @{} fps · payload {} kbps · dec p75 {:.1} ms\nLadder {}p · {} steps ({}) · test {} / cap {}",
        video_codec_label(s.send_codec, s.send_hardware),
        s.send_width,
        s.send_height,
        s.send_fps,
        s.send_payload_bps / 1000,
        s.bitrate_bps / 1000,
        s.encode_ms,
        video_codec_label(s.recv_codec, s.recv_hardware),
        s.recv_width,
        s.recv_height,
        s.recv_fps,
        s.recv_payload_bps / 1000,
        s.decode_p75_ms,
        s.send_rung_height,
        s.ladder_steps,
        s.last_step_reason,
        size,
        cap,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::video_codecs as negotiated_codecs;

    fn codecs() -> codecs::LocalCodecs {
        codecs::LocalCodecs {
            decode: vec![VideoCodec::Av1, VideoCodec::Vp8],
            encode: vec![VideoCodec::Vp8, VideoCodec::Av1],
            hw_decode: vec![VideoCodec::Vp8],
            hw_encode: vec![VideoCodec::Vp8],
        }
    }
    fn offer(c: codecs::LocalCodecs) -> VideoOffer {
        VideoOffer {
            max_pixels: MAX_PIXELS,
            max_fps: 30,
            decode: c.decode,
            encode: c.encode,
            hw_decode: c.hw_decode,
            hw_encode: c.hw_encode,
            hint_flags: true,
        }
    }

    #[test]
    fn force_filters_only_send_and_beats_hardware_score() {
        let mut c = codecs();
        VideoTuning {
            send_codec: "av1".into(),
            ..Default::default()
        }
        .filter(&mut c);
        assert_eq!(c.encode, vec![VideoCodec::Av1]);
        assert!(c.hw_encode.is_empty());
        assert_eq!(c.decode.len(), 2);
        assert_eq!(
            negotiated_codecs(&offer(c), &offer(codecs())),
            Some((VideoCodec::Av1, VideoCodec::Vp8))
        );
    }

    #[test]
    fn no_common_codec_is_not_a_silent_av1_fallback() {
        let mut sender = codecs();
        VideoTuning {
            send_codec: "vp8".into(),
            ..Default::default()
        }
        .filter(&mut sender);
        let mut peer = codecs();
        peer.decode = vec![VideoCodec::Av1];
        let (a, b) = (offer(sender), offer(peer));
        assert_eq!(negotiated_codecs(&a, &b), None);
        assert_eq!(negotiated_codecs(&b, &a), None);
    }

    #[test]
    fn validates_controls_and_local_capability() {
        let mut c = codecs();
        c.encode = vec![VideoCodec::Av1];
        for t in [
            VideoTuning {
                send_codec: "vp8".into(),
                ..Default::default()
            },
            VideoTuning {
                send_codec: "h264".into(),
                ..Default::default()
            },
            VideoTuning {
                max_bps: 42,
                ..Default::default()
            },
            VideoTuning {
                fixed_height: 1080,
                ..Default::default()
            },
        ] {
            assert!(t.validate(&c).is_err());
        }
        assert!(VideoTuning::default().validate(&c).is_ok());
    }

    #[test]
    fn changing_settings_does_not_mutate_an_existing_offer_or_call() {
        let settings = NodeVideo::default();
        let first = CallVideo::default();
        let original = settings.offer(&first.pipe).unwrap();
        let next = VideoTuning {
            send_codec: "av1".into(),
            max_bps: 1_000_000,
            fixed_height: 360,
        };
        *settings.tuning.lock() = next.clone();
        assert_eq!(first.pipe.stats.lock().tuning, VideoTuning::default());
        let second = CallVideo::default();
        let new_offer = settings.offer(&second.pipe).unwrap();
        assert_eq!(new_offer.encode, vec![VideoCodec::Av1]);
        assert_eq!(second.pipe.stats.lock().tuning, next);
        assert_eq!(original.encode, codecs::local_offer_codecs().encode);
        // Forcing a codec never hides that our sender reads sharper-video hint flags.
        assert!(original.hint_flags && new_offer.hint_flags);
        settings.enabled.store(false, Ordering::SeqCst);
        assert!(settings.offer(&Pipe::default()).is_none());
    }

    #[test]
    fn readout_distinguishes_measured_payload_rate_from_target() {
        let s = VideoStats {
            send_payload_bps: 730_000,
            bitrate_bps: 1_000_000,
            decode_p75_ms: 12.5,
            send_rung_height: 540,
            ..Default::default()
        };
        let label = video_tuning_stats_label(s);
        assert!(label.contains("payload 730 kbps (target 1000)"));
        assert!(label.contains("dec p75 12.5 ms"));
        assert!(label.contains("Ladder 540p"));
        assert_eq!(
            video_tuning_stats_label(VideoStats {
                tuning_problem: "unsupported".into(),
                ..Default::default()
            }),
            "unsupported"
        );
    }

    #[test]
    fn defaults_leave_sender_untouched_and_rate_is_only_a_cap() {
        let mut cfg = SenderConfig::default();
        VideoTuning::default().configure(&mut cfg);
        assert_eq!(cfg.max_bps, 3_000_000);
        assert_eq!(cfg.ladder.fixed_rung, None);
        VideoTuning {
            max_bps: 1_000_000,
            fixed_height: 540,
            ..Default::default()
        }
        .configure(&mut cfg);
        assert_eq!(cfg.max_bps, 1_000_000);
        assert_eq!(cfg.start_bps, 500_000);
        assert_eq!(cfg.ladder.fixed_rung, Some(2));
        assert!(cfg.ladder.rungs.iter().all(|r| r.max_bps == 1_000_000));
    }
}
